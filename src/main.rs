#![allow(unused)]

mod action;
mod app;
mod event;
mod input;
mod state;
mod theme;
mod ui;
mod vfs;

use std::io;
use std::panic;

use crossterm::event::KeyCode;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use crate::action::Action;
use crate::app::App;
use crate::event::{AppEvent, EventHandler};

/// RAII guard that restores the terminal state on drop.
/// This ensures the terminal is always restored, even on panic.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}

fn setup_terminal() -> anyhow::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(crossterm::cursor::Hide)?;
    stdout.execute(crossterm::event::EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal() -> anyhow::Result<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(LeaveAlternateScreen)?;
    stdout.execute(crossterm::cursor::Show)?;
    stdout.execute(crossterm::event::DisableMouseCapture)?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Set up panic hook to restore terminal before printing panic message
    let default_panic = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_panic(info);
    }));

    // Initialize terminal
    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard; // RAII — will restore terminal on drop

    // Create the action channel
    let (action_tx, mut action_rx) = tokio::sync::mpsc::unbounded_channel::<Action>();

    // Determine starting directory
    let start_dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()));

    let start_dir = std::fs::canonicalize(&start_dir).unwrap_or(start_dir);

    // Initialize app state
    let fs_watcher = match vfs::watcher::FsWatcher::new(action_tx.clone()) {
        Ok(w) => Some(w),
        Err(_) => None, // Graceful degradation — no live reload if watcher fails
    };
    let mut app = App::new(start_dir, action_tx.clone(), fs_watcher);

    // Start the event handler
    let mut event_handler = EventHandler::new(50); // ~20 ticks/sec

    // Initial directory load
    app.request_directory_load();

    // ── Main Event Loop ─────────────────────────────────────────
    loop {
        // Render
        terminal.draw(|frame| {
            ui::render(&mut app, frame);
        })?;

        // Wait for next event
        let event = event_handler.next().await?;

        match event {
            AppEvent::Key(key) => {
                // Help menu intercepts all keys — any key dismisses it
                if app.show_help {
                    app.dispatch(Action::HideHelp);
                }
                // Handle confirmation dialog keys first
                else if app.show_confirm_dialog {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.dispatch(Action::ConfirmDelete);
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            app.dispatch(Action::CancelDialog);
                        }
                        _ => {} // Ignore other keys
                    }
                }
                // Handle text input modes
                else if app.mode.is_input_mode() {
                    if !app.handle_text_input(key) {
                        // Key wasn't consumed by text input — try action mapping
                        if let Some(action) = input::handler::handle_key_event(&app.mode, key) {
                            app.dispatch(action);
                        }
                    }
                }
                // Handle normal/visual mode keys
                else if let Some(action) = input::handler::handle_key_event(&app.mode, key) {
                    app.dispatch(action);
                }
            }
            AppEvent::Tick => {
                app.dispatch(Action::Tick);
            }
            AppEvent::Resize(w, h) => {
                app.dispatch(Action::Resize(w, h));
            }
        }

        // Drain async results from the channel
        while let Ok(action) = action_rx.try_recv() {
            app.dispatch(action);
        }

        // Check for quit
        if app.should_quit {
            break;
        }
    }

    Ok(())
}
