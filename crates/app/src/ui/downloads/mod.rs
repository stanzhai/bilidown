//! Downloads pivot + task dialogs (ticket 22): task list, bulk actions,
//! duplicate/exit/log-viewer dialogs. View-model logic in `model`,
//! log-viewer state in `log_view`, gpui surface in `screen`/`dialogs`.
//!
//! `open_exit_dialog` + `apply_exit_choice` are the window-close seam
//! ticket 26 wires into main.rs; `open_duplicate_dialog` +
//! `apply_duplicate_choice` the download-start seam.

pub mod dialogs;
pub mod log_view;
pub mod model;
pub mod screen;
pub mod screen_state;

use std::path::{Path, PathBuf};

/// Open `dir` in the platform file manager.
///
/// Two Windows traps live here: `explorer.exe` silently opens Documents
/// when handed a path it can't resolve — relative, or not yet existing
/// (a queued task's `download_path/folder` is only created on first
/// write) — and arguments must stay `OsStr`; `to_string_lossy()` would
/// mangle non-UTF-8 CJK paths. macOS/Linux get the same absolute-path
/// guarantee so `open`/`xdg-open` behave identically.
pub fn open_directory(dir: &Path) -> std::io::Result<()> {
    let dir = nearest_existing(dir)?;
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(&dir).spawn();
    status.map(|_| ())
}

/// Longest existing ancestor of `dir` (itself when it exists). Absolute
/// via [`std::path::absolute`] — unlike `canonicalize` it adds no
/// `\\?\` UNC prefix, which explorer can't open.
fn nearest_existing(dir: &Path) -> std::io::Result<PathBuf> {
    let mut d = std::path::absolute(dir)?;
    while !d.exists() {
        if !d.pop() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no existing ancestor: {}", dir.display()),
            ));
        }
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_existing_walks_up_to_real_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&real).unwrap();
        // Deep non-existent tail resolves to the existing ancestor…
        assert_eq!(
            nearest_existing(&real.join("c").join("d")).unwrap(),
            std::path::absolute(&real).unwrap()
        );
        // …and an existing dir resolves to itself.
        assert_eq!(
            nearest_existing(&real).unwrap(),
            std::path::absolute(&real).unwrap()
        );
    }
}
