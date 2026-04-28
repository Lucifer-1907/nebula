use std::path::PathBuf;

use crate::vfs::entry::FileEntry;
use crate::ui::preview::PreviewContent;

/// Every state mutation in Nebula flows through this single enum.
/// This makes the data flow unidirectional and easy to reason about.
#[derive(Debug, Clone)]
pub enum Action {
    // ── Navigation ──────────────────────────────────────────────
    MoveUp,
    MoveDown,
    MoveTop,
    MoveBottom,
    PageUp,
    PageDown,
    Enter,
    GoBack,

    // ── Mode Transitions ────────────────────────────────────────
    EnterVisual,
    ExitVisual,
    EnterCommand,
    ExitCommand,

    // ── Selection (Visual mode) ─────────────────────────────────
    ToggleSelect,
    SelectAll,
    ClearSelection,

    // ── CRUD Operations ─────────────────────────────────────────
    PromptCreateFile,
    PromptCreateDir,
    PromptRename,
    PromptDelete,
    ConfirmDelete,
    CancelDialog,
    SubmitInput(String),

    // ── Sorting ─────────────────────────────────────────────────
    CycleSortMode,

    // ── Toggle ──────────────────────────────────────────────────
    ToggleHidden,

    // ── Help Menu ───────────────────────────────────────────────
    ShowHelp,
    HideHelp,

    // ── Async Results (posted from tokio workers) ───────────────
    DirectoryLoaded {
        path: PathBuf,
        entries: Vec<FileEntry>,
    },
    PreviewLoaded {
        path: PathBuf,
        content: PreviewContent,
    },
    OperationComplete {
        message: String,
    },
    OperationError {
        message: String,
    },

    // ── Filesystem Watcher ────────────────────────────────────────
    /// Triggered by the inotify/kqueue watcher when files change externally.
    RefreshDir,

    // ── UI ──────────────────────────────────────────────────────
    Tick,
    Resize(u16, u16),
    Quit,
}
