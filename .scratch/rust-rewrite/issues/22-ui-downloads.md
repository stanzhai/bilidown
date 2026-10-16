# 22: Downloads pivot + task dialogs

**What to build:** The downloads screen: task list with cover, title,
progress, speed, status; start-all/pause-all/delete-all; sort/filter;
duplicate-resolution dialog (ask/skip/continue); exit dialog
(minimize-to-tray vs exit, remember); log viewer dialog wired to the log
backend.

**Blocked by:** 16, 20, 19.

**Status:** resolved

- [x] List reflects live task state from the manager (progress/speed/status)
- [x] Bulk actions and sort/filter behave as Python's
- [x] Duplicate dialog offers ask/skip/continue and feeds the manager
- [x] Exit dialog with remember-choice; log viewer with filter

## Answer

Implemented in `crates/app/src/ui/downloads/` (model/screen/screen_state/dialogs/log_view) + `ui/fmt.rs`; `Shell` owns the TaskManager and renders `DownloadsScreen`; `Shell::ask_exit` is the ticket-26 close-flow seam. Commit: fe3c14f5 (merged 1a7466b6).

## Comments
