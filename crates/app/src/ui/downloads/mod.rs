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
