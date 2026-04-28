use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;

/// Handle to the filesystem watcher.
/// Keeps the watcher alive and allows dynamic path switching.
pub struct FsWatcher {
    /// The underlying notify watcher (wrapped in Arc<Mutex> for thread safety).
    watcher: Arc<Mutex<RecommendedWatcher>>,
    /// The currently watched path (so we can unwatch before switching).
    current_path: Arc<Mutex<Option<PathBuf>>>,
}

impl FsWatcher {
    /// Create a new filesystem watcher that sends `RefreshDir` actions
    /// through the provided channel when files change.
    ///
    /// Uses a debounce approach: events within 300ms are coalesced into
    /// a single refresh to prevent UI flickering from rapid changes
    /// (e.g., extracting an archive).
    pub fn new(tx: UnboundedSender<Action>) -> anyhow::Result<Self> {
        // We use a std channel to bridge notify's sync callback to our async world
        let (notify_tx, notify_rx) = std::sync::mpsc::channel::<Event>();

        // Create the watcher with a custom config for responsiveness
        let watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| {
                if let Ok(event) = result {
                    // Only care about modification events that affect directory listings
                    match event.kind {
                        EventKind::Create(_)
                        | EventKind::Remove(_)
                        | EventKind::Modify(notify::event::ModifyKind::Name(_))
                        | EventKind::Modify(notify::event::ModifyKind::Data(_)) => {
                            let _ = notify_tx.send(event);
                        }
                        _ => {
                            // Ignore metadata-only changes, access events, etc.
                        }
                    }
                }
            },
            Config::default()
                .with_poll_interval(Duration::from_secs(2)),
        )?;

        let watcher = Arc::new(Mutex::new(watcher));
        let current_path = Arc::new(Mutex::new(None::<PathBuf>));

        // Spawn a background thread that debounces notify events and sends
        // RefreshDir actions. This prevents rapid filesystem changes from
        // causing dozens of reloads per second.
        let action_tx = tx;
        std::thread::spawn(move || {
            debounce_loop(notify_rx, action_tx);
        });

        Ok(Self {
            watcher,
            current_path,
        })
    }

    /// Switch the watcher to monitor a new directory.
    /// Unwatches the previous path (if any) and starts watching the new one.
    pub fn watch(&self, new_path: &PathBuf) {
        let mut watcher = match self.watcher.lock() {
            Ok(w) => w,
            Err(_) => return, // Mutex poisoned — silently ignore
        };
        let mut current = match self.current_path.lock() {
            Ok(c) => c,
            Err(_) => return,
        };

        // Unwatch the old path
        if let Some(ref old_path) = *current {
            let _ = watcher.unwatch(old_path.as_path());
        }

        // Watch the new path (non-recursive — we only care about direct children)
        let _ = watcher.watch(new_path.as_path(), RecursiveMode::NonRecursive);

        *current = Some(new_path.clone());
    }
}

/// Debounce loop running on a background thread.
/// Coalesces rapid filesystem events into a single `RefreshDir` action
/// with a 300ms quiet period.
fn debounce_loop(
    rx: std::sync::mpsc::Receiver<Event>,
    tx: UnboundedSender<Action>,
) {
    let debounce_duration = Duration::from_millis(300);

    loop {
        // Block until we get the first event
        match rx.recv() {
            Ok(_) => {
                // Drain any events that arrive within the debounce window
                loop {
                    match rx.recv_timeout(debounce_duration) {
                        Ok(_) => {
                            // More events arriving — keep draining
                            continue;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            // Quiet period reached — send one refresh
                            break;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            return; // Watcher dropped — exit thread
                        }
                    }
                }

                // Send a single debounced refresh action
                if tx.send(Action::RefreshDir).is_err() {
                    return; // Receiver dropped — exit thread
                }
            }
            Err(_) => {
                // Sender (watcher) dropped — exit thread
                return;
            }
        }
    }
}
