# 44: Download options dialog gates task queueing

**What to fix:** "下载所选项" (Download Selected Items) on the parse screen
queued tasks and switched to the Downloads screen immediately, before the
download options dialog was answered. Queueing must happen only after the
user presses the dialog's Download button; Cancel must queue nothing.

**Status:** resolved

## Answer
`ParseScreen::on_download` now consults a pure `download_gate(cfg)` rule
(`Behavior/show_download_options_dialog`, default on → `Dialog`; missing
config store or flag explicitly off → `Direct`). The Dialog branch opens
the options dialog and returns 0 — no queue, no screen switch; the shared
queueing tail lives in `ParseScreen::queue_checked` and runs from an
`on_accept` continuation that `open_download_options` fires only after its
footer Download button commits and the dialog closes (Cancel never fires
it, one-shot `Rc<RefCell<Option<F>>>` + `take()`). The Direct path calls
`queue_checked` immediately as before. Regression test
`download_gate_dialog_by_default_and_direct_when_disabled` in
`app/src/ui/parse/screen.rs` pins the gate rule.
