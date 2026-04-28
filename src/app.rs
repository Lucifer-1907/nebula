use std::path::PathBuf;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::action::Action;
use crate::state::mode::Mode;
use crate::state::selection::Selection;
use crate::state::tab::Tab;
use crate::ui::preview::PreviewContent;
use crate::vfs::watcher::FsWatcher;

/// The kind of input prompt currently displayed.
#[derive(Debug, Clone)]
pub enum InputPrompt {
    CreateFile,
    CreateDir,
    Rename(PathBuf), // original path being renamed
}

impl std::fmt::Display for InputPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputPrompt::CreateFile => write!(f, "Create File"),
            InputPrompt::CreateDir => write!(f, "Create Directory"),
            InputPrompt::Rename(_) => write!(f, "Rename"),
        }
    }
}

/// Central application state. All state lives here.
pub struct App {
    /// Current editing mode.
    pub mode: Mode,

    /// The active tab (we support one tab in Phase 1).
    tabs: Vec<Tab>,
    active_tab: usize,

    /// Global show hidden files toggle — applied uniformly to all columns.
    pub show_hidden: bool,

    /// Visual-mode selection.
    pub selection: Selection,

    /// Command-mode input buffer.
    pub command_input: String,

    /// Input-mode state.
    pub input_prompt: Option<InputPrompt>,
    pub input_buffer: String,

    /// Confirmation dialog state.
    pub show_confirm_dialog: bool,
    pub confirm_message: String,
    pub pending_delete_paths: Vec<PathBuf>,

    /// Help menu visibility.
    pub show_help: bool,

    /// Ephemeral status message with auto-dismiss.
    pub status_message: Option<String>,
    pub status_is_error: bool,
    status_time: Option<Instant>,

    /// Channel to send actions (for async task results).
    pub action_tx: UnboundedSender<Action>,

    /// Animation tick counter.
    pub tick_count: u64,

    /// Should we quit?
    pub should_quit: bool,

    /// Track the last path we loaded preview for, to avoid duplicate requests.
    last_preview_path: Option<PathBuf>,

    /// Cancellation token for the current preview task.
    /// When a new preview is requested, the previous token is cancelled
    /// so stale I/O tasks don't pile up during rapid scrolling.
    preview_cancel: Option<CancellationToken>,

    /// Filesystem watcher — monitors the current directory for external changes.
    fs_watcher: Option<FsWatcher>,
}

impl App {
    pub fn new(start_dir: PathBuf, action_tx: UnboundedSender<Action>, fs_watcher: Option<FsWatcher>) -> Self {
        let tab = Tab::new(start_dir);

        Self {
            mode: Mode::Normal,
            tabs: vec![tab],
            active_tab: 0,
            show_hidden: false,
            selection: Selection::new(),
            command_input: String::new(),
            input_prompt: None,
            input_buffer: String::new(),
            show_confirm_dialog: false,
            confirm_message: String::new(),
            pending_delete_paths: Vec::new(),
            show_help: false,
            status_message: None,
            status_is_error: false,
            status_time: None,
            action_tx,
            tick_count: 0,
            should_quit: false,
            last_preview_path: None,
            preview_cancel: None,
            fs_watcher,
        }
    }

    /// Get a reference to the active tab.
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// Get a mutable reference to the active tab.
    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }

    /// Get selected paths (from selection set, or fallback to current entry).
    pub fn selection_paths(&self) -> Vec<PathBuf> {
        if self.selection.is_empty() {
            if let Some(entry) = self.tab().current_entry() {
                vec![entry.path.clone()]
            } else {
                Vec::new()
            }
        } else {
            self.selection.paths()
        }
    }

    /// Called on each tick — auto-dismiss status messages, advance animations.
    pub fn on_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);

        // Auto-dismiss status messages after 3 seconds
        if let Some(time) = self.status_time {
            if time.elapsed().as_secs() >= 3 {
                self.status_message = None;
                self.status_is_error = false;
                self.status_time = None;
            }
        }
    }

    /// Set an ephemeral status message.
    fn set_status(&mut self, msg: String, is_error: bool) {
        self.status_message = Some(msg);
        self.status_is_error = is_error;
        self.status_time = Some(Instant::now());
    }

    /// Request preview load for the currently highlighted entry.
    /// Cancels any previous in-flight preview task first.
    pub fn request_preview(&mut self) {
        let preview_path = self.tab().current_entry().map(|e| e.path.clone());

        if preview_path == self.last_preview_path {
            return; // Already loaded or loading
        }

        // Cancel the previous preview task (if any) so stale I/O doesn't pile up
        if let Some(cancel) = self.preview_cancel.take() {
            cancel.cancel();
        }

        self.last_preview_path = preview_path.clone();

        if let Some(path) = preview_path {
            self.tab_mut().preview_content = PreviewContent::Loading;
            let tx = self.action_tx.clone();
            let cancel = CancellationToken::new();
            self.preview_cancel = Some(cancel.clone());
            let show_hidden = self.show_hidden;
            tokio::spawn(crate::vfs::scanner::load_preview(path, tx, cancel, show_hidden));
        } else {
            self.tab_mut().preview_content = PreviewContent::Empty;
        }
    }

    /// Request loading of the current and parent directories.
    pub fn request_directory_load(&mut self) {
        let current_dir = self.tab().current_dir.clone();

        // Update the filesystem watcher to monitor the new directory
        if let Some(ref watcher) = self.fs_watcher {
            watcher.watch(&current_dir);
        }

        // Load current directory
        let tx = self.action_tx.clone();
        let path = current_dir.clone();
        tokio::spawn(crate::vfs::scanner::scan_directory(path, tx));

        // Load parent directory
        if let Some(parent) = current_dir.parent() {
            let tx = self.action_tx.clone();
            let parent_path = parent.to_path_buf();
            tokio::spawn(async move {
                let mut entries = Vec::new();
                if let Ok(mut read_dir) = tokio::fs::read_dir(&parent_path).await {
                    while let Ok(Some(dir_entry)) = read_dir.next_entry().await {
                        if let Ok(entry) = crate::vfs::entry::FileEntry::from_dir_entry(dir_entry).await {
                            entries.push(entry);
                        }
                    }
                }
                let _ = tx.send(Action::DirectoryLoaded {
                    path: parent_path,
                    entries,
                });
            });
        }

        // Reset preview — force re-load
        if let Some(cancel) = self.preview_cancel.take() {
            cancel.cancel();
        }
        self.last_preview_path = None;
    }

    /// Process an action and mutate state accordingly.
    pub fn dispatch(&mut self, action: Action) {
        // ── Help menu intercepts everything ─────────────────────
        if self.show_help {
            match action {
                Action::HideHelp | Action::Quit => {
                    self.show_help = false;
                }
                // Any key dismisses the help menu
                Action::MoveUp | Action::MoveDown | Action::Enter
                | Action::GoBack | Action::ExitCommand | Action::CancelDialog => {
                    self.show_help = false;
                }
                Action::Tick => self.on_tick(),
                Action::Resize(_, _) => {}
                _ => {
                    self.show_help = false;
                }
            }
            return;
        }

        // ── Confirm dialog intercepts everything ────────────────
        if self.show_confirm_dialog {
            match action {
                Action::ConfirmDelete => {
                    self.show_confirm_dialog = false;
                    let paths = std::mem::take(&mut self.pending_delete_paths);
                    if !paths.is_empty() {
                        let tx = self.action_tx.clone();
                        tokio::spawn(crate::vfs::ops::delete(paths, tx));
                    }
                    return;
                }
                Action::CancelDialog => {
                    self.show_confirm_dialog = false;
                    self.pending_delete_paths.clear();
                    self.confirm_message.clear();
                    return;
                }
                Action::Tick => {
                    self.on_tick();
                    return;
                }
                _ => return, // Ignore everything else when dialog is open
            }
        }

        match action {
            // ── Navigation ──────────────────────────────────────
            Action::MoveUp => {
                self.tab_mut().move_up();
                if self.mode == Mode::Visual {
                    let path = self.tab().current_entry().map(|e| e.path.clone());
                    if let Some(p) = path {
                        self.selection.select(p);
                    }
                }
                self.request_preview();
            }
            Action::MoveDown => {
                self.tab_mut().move_down();
                if self.mode == Mode::Visual {
                    let path = self.tab().current_entry().map(|e| e.path.clone());
                    if let Some(p) = path {
                        self.selection.select(p);
                    }
                }
                self.request_preview();
            }
            Action::MoveTop => {
                self.tab_mut().move_top();
                self.request_preview();
            }
            Action::MoveBottom => {
                self.tab_mut().move_bottom();
                self.request_preview();
            }
            Action::PageUp => {
                self.tab_mut().page_up(20);
                self.request_preview();
            }
            Action::PageDown => {
                self.tab_mut().page_down(20);
                self.request_preview();
            }

            Action::Enter => {
                if let Some(entry) = self.tab().current_entry().cloned() {
                    if entry.is_broken_symlink {
                        self.set_status(
                            format!("Cannot open broken symlink: {}", entry.name),
                            true,
                        );
                    } else if entry.is_dir {
                        // Save cursor position before navigating
                        self.tab_mut().save_cursor();
                        let new_dir = entry.path.clone();
                        self.tab_mut().current_dir = new_dir;
                        self.request_directory_load();
                    } else {
                        // Open file with OS default application
                        crate::vfs::ops::open_file(&entry.path, &self.action_tx);
                    }
                }
            }

            Action::GoBack => {
                let current = self.tab().current_dir.clone();
                if let Some(parent) = current.parent() {
                    self.tab_mut().save_cursor();
                    self.tab_mut().current_dir = parent.to_path_buf();
                    self.request_directory_load();
                }
            }

            // ── Mode Transitions ────────────────────────────────
            Action::EnterVisual => {
                self.mode = Mode::Visual;
                self.selection.clear();
                let entry_data = self.tab().current_entry().map(|e| e.path.clone());
                let anchor = self.tab().cursor_index();
                if let Some(p) = entry_data {
                    self.selection.select(p);
                    self.selection.set_anchor(anchor);
                }
            }
            Action::ExitVisual => {
                self.mode = Mode::Normal;
                self.selection.clear();
            }
            Action::EnterCommand => {
                self.mode = Mode::Command;
                self.command_input.clear();
            }
            Action::ExitCommand => {
                self.mode = Mode::Normal;
                self.command_input.clear();
            }

            // ── Help Menu ───────────────────────────────────────
            Action::ShowHelp => {
                self.show_help = true;
            }
            Action::HideHelp => {
                self.show_help = false;
            }

            // ── Selection ───────────────────────────────────────
            Action::ToggleSelect => {
                let path = self.tab().current_entry().map(|e| e.path.clone());
                if let Some(p) = path {
                    self.selection.toggle(&p);
                }
                // Move down after toggling (yazi-like behavior)
                self.tab_mut().move_down();
                self.request_preview();
            }
            Action::SelectAll => {
                let paths: Vec<PathBuf> = self.tab().current_entries
                    .iter()
                    .map(|e| e.path.clone())
                    .collect();
                for path in paths {
                    self.selection.select(path);
                }
            }
            Action::ClearSelection => {
                self.selection.clear();
            }

            // ── CRUD Prompts ────────────────────────────────────
            Action::PromptCreateFile => {
                self.mode = Mode::Input;
                self.input_prompt = Some(InputPrompt::CreateFile);
                self.input_buffer.clear();
            }
            Action::PromptCreateDir => {
                self.mode = Mode::Input;
                self.input_prompt = Some(InputPrompt::CreateDir);
                self.input_buffer.clear();
            }
            Action::PromptRename => {
                let entry_info = self.tab().current_entry().map(|e| (e.path.clone(), e.name.clone()));
                if let Some((path, name)) = entry_info {
                    self.mode = Mode::Input;
                    self.input_prompt = Some(InputPrompt::Rename(path));
                    self.input_buffer = name;
                }
            }
            Action::PromptDelete => {
                let paths = self.selection_paths();
                if paths.is_empty() {
                    return;
                }

                // Verify paths still exist before prompting
                let valid_paths: Vec<PathBuf> = paths
                    .into_iter()
                    .filter(|p| p.exists() || p.symlink_metadata().is_ok())
                    .collect();

                if valid_paths.is_empty() {
                    self.set_status("No valid items to delete (may have been removed externally)".to_string(), true);
                    self.request_directory_load(); // Refresh to remove stale entries
                    return;
                }

                let count = valid_paths.len();
                self.confirm_message = if count == 1 {
                    format!(
                        "Delete \"{}\"?",
                        valid_paths[0].file_name().unwrap_or_default().to_string_lossy()
                    )
                } else {
                    format!("Delete {} items?", count)
                };
                self.pending_delete_paths = valid_paths;
                self.show_confirm_dialog = true;
            }
            Action::ConfirmDelete => {
                // Handled at the top of dispatch
            }
            Action::CancelDialog => {
                self.mode = Mode::Normal;
                self.input_prompt = None;
                self.input_buffer.clear();
                self.show_confirm_dialog = false;
                self.pending_delete_paths.clear();
            }

            Action::SubmitInput(value) => {
                if let Some(prompt) = self.input_prompt.take() {
                    let current_dir = self.tab().current_dir.clone();
                    let tx = self.action_tx.clone();

                    match prompt {
                        InputPrompt::CreateFile => {
                            let path = current_dir.join(&value);
                            tokio::spawn(crate::vfs::ops::create_file(path, tx));
                        }
                        InputPrompt::CreateDir => {
                            let path = current_dir.join(&value);
                            tokio::spawn(crate::vfs::ops::create_dir(path, tx));
                        }
                        InputPrompt::Rename(original) => {
                            // Handle case where original was deleted externally
                            if !original.exists() && original.symlink_metadata().is_err() {
                                self.set_status("Cannot rename — file no longer exists".to_string(), true);
                                self.mode = Mode::Normal;
                                self.input_buffer.clear();
                                self.request_directory_load();
                                return;
                            }
                            let new_path = current_dir.join(&value);
                            tokio::spawn(crate::vfs::ops::rename(original, new_path, tx));
                        }
                    }
                }
                self.mode = Mode::Normal;
                self.input_buffer.clear();
            }

            // ── Toggles ────────────────────────────────────────
            Action::CycleSortMode => {
                let new_mode = self.tab().sort_mode.next();
                self.tab_mut().sort_mode = new_mode;
                // Re-sort in place
                let mut entries = std::mem::take(&mut self.tab_mut().current_entries);
                new_mode.sort(&mut entries);
                self.tab_mut().current_entries = entries;
                self.set_status(format!("Sort: {}", new_mode), false);
            }
            Action::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.set_status(
                    format!("Hidden files: {}", if self.show_hidden { "shown" } else { "hidden" }),
                    false,
                );
                // Reload to apply filter uniformly across all columns + preview
                self.request_directory_load();
            }

            // ── Async Results ───────────────────────────────────
            Action::DirectoryLoaded { path, entries } => {
                let current_dir = self.tab().current_dir.clone();
                let show_hidden = self.show_hidden;

                if path == current_dir {
                    // Current directory loaded
                    self.tab_mut().set_current_entries(entries, show_hidden);
                    self.tab_mut().restore_cursor(&path);
                    self.request_preview();
                } else if Some(path.as_path()) == current_dir.parent() {
                    // Parent directory loaded
                    self.tab_mut().set_parent_entries(entries, show_hidden);
                }
            }

            Action::PreviewLoaded { path, content } => {
                // Only apply if this is still the relevant preview
                let matches = self.tab().current_entry().map(|e| e.path.clone()) == Some(path);
                if matches {
                    self.tab_mut().preview_content = content;
                }
            }

            Action::OperationComplete { message } => {
                self.set_status(message, false);
                self.selection.clear();
                // Reload directory to reflect changes
                self.request_directory_load();
            }
            Action::OperationError { message } => {
                self.set_status(message, true);
            }

            // ── Filesystem Watcher ───────────────────────────────
            Action::RefreshDir => {
                // External filesystem change detected — silently reload
                // without showing a status message (it's automatic).
                let current_dir = self.tab().current_dir.clone();
                let tx = self.action_tx.clone();
                tokio::spawn(crate::vfs::scanner::scan_directory(current_dir, tx));
                // Also reset preview in case the highlighted entry changed
                if let Some(cancel) = self.preview_cancel.take() {
                    cancel.cancel();
                }
                self.last_preview_path = None;
            }

            // ── UI ──────────────────────────────────────────────
            Action::Tick => {
                self.on_tick();
            }
            Action::Resize(_, _) => {
                // ratatui handles resize automatically
            }
            Action::Quit => {
                self.should_quit = true;
            }
        }
    }

    /// Handle raw key events for text input modes (Command / Input).
    /// Returns true if the key was consumed.
    pub fn handle_text_input(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        match self.mode {
            Mode::Command => match key.code {
                KeyCode::Char(c) => {
                    self.command_input.push(c);
                    true
                }
                KeyCode::Backspace => {
                    self.command_input.pop();
                    if self.command_input.is_empty() {
                        self.mode = Mode::Normal;
                    }
                    true
                }
                KeyCode::Enter => {
                    let cmd = self.command_input.clone();
                    self.mode = Mode::Normal;
                    self.command_input.clear();
                    self.execute_command(&cmd);
                    true
                }
                _ => false,
            },
            Mode::Input => match key.code {
                KeyCode::Char(c) => {
                    self.input_buffer.push(c);
                    true
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                    true
                }
                KeyCode::Enter => {
                    let value = self.input_buffer.clone();
                    if !value.is_empty() {
                        self.dispatch(Action::SubmitInput(value));
                    }
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Execute a command-mode command.
    fn execute_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
        match parts.first().copied() {
            Some("q" | "quit") => {
                self.should_quit = true;
            }
            Some("mkdir") => {
                if let Some(name) = parts.get(1) {
                    let path = self.tab().current_dir.join(name);
                    let tx = self.action_tx.clone();
                    tokio::spawn(crate::vfs::ops::create_dir(path, tx));
                } else {
                    self.set_status("Usage: mkdir <name>".to_string(), true);
                }
            }
            Some("touch") => {
                if let Some(name) = parts.get(1) {
                    let path = self.tab().current_dir.join(name);
                    let tx = self.action_tx.clone();
                    tokio::spawn(crate::vfs::ops::create_file(path, tx));
                } else {
                    self.set_status("Usage: touch <name>".to_string(), true);
                }
            }
            Some("sort") => {
                self.dispatch(Action::CycleSortMode);
            }
            Some("hidden") => {
                self.dispatch(Action::ToggleHidden);
            }
            Some("help") => {
                self.show_help = true;
            }
            Some(unknown) => {
                self.set_status(format!("Unknown command: {}", unknown), true);
            }
            None => {}
        }
    }
}
