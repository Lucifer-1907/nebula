use std::path::PathBuf;

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::action::Action;
use crate::vfs::entry::FileEntry;

/// Asynchronously scan a directory and post results back via channel.
/// This runs on the tokio runtime so the UI thread never blocks.
pub async fn scan_directory(path: PathBuf, tx: UnboundedSender<Action>) {
    match scan_inner(&path).await {
        Ok(entries) => {
            let _ = tx.send(Action::DirectoryLoaded { path, entries });
        }
        Err(e) => {
            let _ = tx.send(Action::OperationError {
                message: format!("Failed to read {}: {}", path.display(), e),
            });
        }
    }
}

async fn scan_inner(path: &PathBuf) -> anyhow::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(path).await?;

    while let Some(dir_entry) = read_dir.next_entry().await? {
        match FileEntry::from_dir_entry(dir_entry).await {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                // Skip entries we can't read (permission denied, etc.)
                continue;
            }
        }
    }

    Ok(entries)
}

/// Load preview content for a path.
/// - Directories: scan and list their entries.
/// - Files: return Empty (blank pane — no file content preview).
/// Accepts a `CancellationToken` — if cancelled (user scrolled past), the task
/// stops early without posting stale results.
/// `show_hidden` filters directory previews to match the global setting.
pub async fn load_preview(
    path: PathBuf,
    tx: UnboundedSender<Action>,
    cancel: CancellationToken,
    show_hidden: bool,
    picker: Option<ratatui_image::picker::Picker>,
) {
    use crate::ui::preview::PreviewContent;

    // Check cancellation before starting I/O
    if cancel.is_cancelled() {
        return;
    }

    let content = if path.is_dir() {
        // Directory preview: list its entries
        match scan_inner(&path).await {
            Ok(mut entries) => {
                // Check cancellation after scan
                if cancel.is_cancelled() {
                    return;
                }

                // Apply global show_hidden filter
                if !show_hidden {
                    entries.retain(|e| !e.is_hidden);
                }

                // Sort: dirs first, then alphabetical
                entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });

                if entries.is_empty() {
                    PreviewContent::EmptyDir
                } else {
                    PreviewContent::Directory(entries)
                }
            }
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("Permission denied") {
                    PreviewContent::PermissionDenied
                } else {
                    PreviewContent::Error(msg)
                }
            }
        }
    } else if path.is_symlink() && tokio::fs::metadata(&path).await.is_err() {
        // Broken symlink
        PreviewContent::Error("Broken symlink — target does not exist".to_string())
    } else {
        // File selected — check if it's an image
        let is_image = path.extension().and_then(|e| e.to_str()).map_or(false, |ext| {
            matches!(
                ext.to_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "ico" | "tiff"
            )
        });

        if is_image {
            if let Some(mut picker) = picker {
                let img_path = path.clone();
                let cancel_clone = cancel.clone();
                
                // Decode image in a blocking task so we don't block the async runtime
                let result = tokio::task::spawn_blocking(move || -> Result<ratatui_image::protocol::StatefulProtocol, String> {
                    if cancel_clone.is_cancelled() {
                        return Err("Cancelled".to_string());
                    }
                    
                    let dyn_img = match image::open(&img_path) {
                        Ok(img) => img,
                        Err(e) => return Err(format!("Decode error: {}", e)),
                    };
                    
                    if cancel_clone.is_cancelled() {
                        return Err("Cancelled".to_string());
                    }
                    
                    Ok(picker.new_resize_protocol(dyn_img))
                })
                .await;

                match result {
                    Ok(Ok(protocol)) => {
                        use std::sync::{Arc, Mutex};
                        use crate::ui::preview::ImageProtocol;
                        PreviewContent::Image(ImageProtocol(Arc::new(Mutex::new(protocol))))
                    }
                    Ok(Err(msg)) if msg == "Cancelled" => PreviewContent::Empty,
                    Ok(Err(msg)) => PreviewContent::Error(msg),
                    _ => PreviewContent::Empty,
                }
            } else {
                PreviewContent::Empty
            }
        } else {
            PreviewContent::Empty
        }
    };

    // Final cancellation check before sending
    if cancel.is_cancelled() {
        return;
    }

    let _ = tx.send(Action::PreviewLoaded { path, content });
}
