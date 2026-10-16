//! Downloads-screen dialogs (port of `src/gui/dialog/download_options/
//! duplicate.py`, `dialog/exit.py`, `dialog/log.py`).
//!
//! All three open via `WindowExt::open_dialog`; persistent entities
//! (checkbox/radio/log model) are created before opening and cloned into
//! the builder. Localization goes through the i18n catalog with the exact
//! Qt contexts Python used.

use gpui_kit::base::IndexPath;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::select::{SearchableVec, Select, SelectItem, SelectState};
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, AppContext, Context, SharedString, Window};

use super::log_view::{default_log_path, open_logs_directory, LogViewModel};
use super::model::{apply_duplicate_choice, apply_exit_choice};
use crate::ui::Shell;

/// Current language: `Interface.language` config, `Auto` resolving the OS
/// locale (Python `Language.value()` + `QLocale()`).
fn language() -> services::i18n::ResolvedLanguage {
    let raw = config::ConfigStore::open()
        .ok()
        .and_then(|c| c.get("Interface", "language").cloned());
    let lang = match raw.as_ref().and_then(|v| v.as_str()) {
        Some(s) => services::i18n::Language::from_config(s),
        None => services::i18n::Language::Auto,
    };
    lang.resolve()
}

/// One translated string (catalog lookup with Qt source fallback).
pub fn tr(context: &str, source: &str) -> String {
    services::i18n::catalog(language())
        .tr(context, source)
        .to_owned()
}

/// `Translator.TIP_MESSAGES` lookup for row status text.
pub fn tip_message(key: &str) -> Option<String> {
    services::i18n::tip_messages(services::i18n::catalog(language()), key)
}

/// `Translator.ERROR_MESSAGES` lookup for row status text.
pub fn error_message(key: &str) -> Option<String> {
    services::i18n::error_messages(services::i18n::catalog(language()), key)
}

// ---------- DuplicateDownloadDialog ----------

/// Mutable checkbox state shared with a dialog builder (builders re-run,
/// so the checkbox itself is stateless).
pub struct RememberState {
    pub checked: bool,
}

impl RememberState {
    fn new() -> Self {
        Self { checked: false }
    }
}

/// Open the duplicate-download dialog. `on_decision` receives `(skip,
/// remember)`; when remembered, the config write has already happened
/// (Python wrote it in `accept`/`reject`).
pub fn open_duplicate_dialog(
    window: &mut Window,
    cx: &mut Context<Shell>,
    task_title: &str,
    on_decision: impl FnOnce(bool, bool) + 'static,
) {
    let remember = cx.new(|_| RememberState::new());
    // The dialog builder may re-run, so the one-shot callback lives
    // behind an Rc and is taken by whichever button fires first.
    let on_decision = std::rc::Rc::new(std::cell::RefCell::new(Some(on_decision)));
    let title = task_title.to_owned();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let remember_checked = remember.read(cx).checked;
        dialog
            .title(tr(
                "DuplicateDownloadDialog",
                "Duplicate Download Detected",
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .py_2()
                    .child(div().child(tr(
                        "DuplicateDownloadDialog",
                        "A duplicate download task already exists in the download list. Do you want to continue downloading?",
                    )))
                    .child(div().child(
                        tr("DuplicateDownloadDialog", "Task Name: {task_title}")
                            .replace("{task_title}", &title),
                    ))
                    .child(Checkbox::new("dup-never-ask")
                        .checked(remember_checked)
                        .label(tr("DuplicateDownloadDialog", "Don't ask again"))
                        .on_click({
                            let remember = remember.clone();
                            move |checked, _w, cx| {
                                remember.update(cx, |r, _| r.checked = *checked);
                            }
                        })),
            )
            .button_props(
                DialogButtonProps::default()
                    .ok_text(tr("DuplicateDownloadDialog", "Continue"))
                    .cancel_text(tr("DuplicateDownloadDialog", "Skip"))
                    .show_cancel(true)
                    .on_ok({
                        let remember = remember.clone();
                        let on_decision = on_decision.clone();
                        move |_ev, _w, cx| {
                            let remembered = remember.read(cx).checked;
                            persist_duplicate(false, remembered);
                            if let Some(f) = on_decision.borrow_mut().take() {
                                f(false, remembered);
                            }
                            true
                        }
                    })
                    .on_cancel({
                        let remember = remember.clone();
                        let on_decision = on_decision.clone();
                        move |_ev, _w, cx| {
                            let remembered = remember.read(cx).checked;
                            persist_duplicate(true, remembered);
                            if let Some(f) = on_decision.borrow_mut().take() {
                                f(true, remembered);
                            }
                            true
                        }
                    }),
            )
    });
}

fn persist_duplicate(skip: bool, never_ask: bool) {
    if !never_ask {
        return;
    }
    if let Ok(mut cfg) = config::ConfigStore::open() {
        apply_duplicate_choice(&mut cfg, skip, true);
    }
}

// ---------- ExitDialog ----------

pub struct ExitSelection {
    pub exit: bool,
}

/// Open the exit dialog (minimize-to-tray vs exit, remember choice).
/// `on_choice` receives `(exit, remember)`; when remembered, the config
/// write has already happened.
pub fn open_exit_dialog(
    window: &mut Window,
    cx: &mut Context<Shell>,
    on_choice: impl FnOnce(bool, bool) + 'static,
) {
    let selection = cx.new(|_| ExitSelection { exit: true });
    let remember = cx.new(|_| RememberState::new());
    let on_choice = std::rc::Rc::new(std::cell::RefCell::new(Some(on_choice)));
    window.open_dialog(cx, move |dialog, _window, cx| {
        let selected = if selection.read(cx).exit { 0 } else { 1 };
        let remember_checked = remember.read(cx).checked;
        dialog
            .title(tr("ExitDialog", "Close the main window"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .py_2()
                    .child(div().child(tr("ExitDialog", "Choose an action")))
                    .child(
                        RadioGroup::new("exit-choice")
                            .selected_index(Some(selected))
                            .child(tr("ExitDialog", "Exit the program"))
                            .child(tr("ExitDialog", "Minimize to system tray"))
                            .on_change({
                                let selection = selection.clone();
                                move |ix, _w, cx| {
                                    selection.update(cx, |s, _| s.exit = *ix == 0);
                                }
                            }),
                    )
                    .child(
                        Checkbox::new("exit-never-ask")
                            .checked(remember_checked)
                            .label(tr("ExitDialog", "Don't ask again"))
                            .on_click({
                                let remember = remember.clone();
                                move |checked, _w, cx| {
                                    remember.update(cx, |r, _| r.checked = *checked);
                                }
                            }),
                    ),
            )
            .button_props(
                DialogButtonProps::default()
                    .ok_text(tr("ExitDialog", "Exit the program"))
                    .cancel_text(tr("ExitDialog", "Minimize to system tray"))
                    .show_cancel(true)
                    .on_ok({
                        let remember = remember.clone();
                        let on_choice = on_choice.clone();
                        move |_ev, _w, cx| {
                            let remembered = remember.read(cx).checked;
                            persist_exit(true, remembered);
                            if let Some(f) = on_choice.borrow_mut().take() {
                                f(true, remembered);
                            }
                            true
                        }
                    })
                    .on_cancel({
                        let remember = remember.clone();
                        let on_choice = on_choice.clone();
                        move |_ev, _w, cx| {
                            let remembered = remember.read(cx).checked;
                            persist_exit(false, remembered);
                            if let Some(f) = on_choice.borrow_mut().take() {
                                f(false, remembered);
                            }
                            true
                        }
                    }),
            )
    });
}

fn persist_exit(exit: bool, never_ask: bool) {
    if !never_ask {
        return;
    }
    if let Ok(mut cfg) = config::ConfigStore::open() {
        apply_exit_choice(&mut cfg, exit, true);
    }
}

// ---------- LogViewerDialog ----------

#[derive(Clone, PartialEq)]
struct LevelItem {
    title: SharedString,
    level: services::log::LevelFilter,
}

impl SelectItem for LevelItem {
    type Value = services::log::LevelFilter;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.level
    }
}

/// Open the log viewer: level combo + search + refresh/clear/open-dir
/// over `logs/app.log` (`services::log` does parse + filter).
pub fn open_log_viewer(window: &mut Window, cx: &mut Context<Shell>) {
    let path = default_log_path().unwrap_or_else(|| std::path::PathBuf::from("app.log"));
    let model = cx.new(|_| {
        let mut m = LogViewModel::new(path);
        m.refresh();
        m
    });
    let search = cx
        .new(|cx| InputState::new(window, cx).placeholder(tr("LogViewerDialog", "Search logs...")));
    let levels = [
        (services::log::LevelFilter::All, "All"),
        (services::log::LevelFilter::Debug, "Debug"),
        (services::log::LevelFilter::Info, "Info"),
        (services::log::LevelFilter::Warning, "Warning"),
        (services::log::LevelFilter::Error, "Error"),
    ]
    .into_iter()
    .map(|(level, key)| LevelItem {
        title: tr("LogViewerDialog", key).into(),
        level,
    })
    .collect::<Vec<_>>();
    let level_select = cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(levels),
            Some(IndexPath::new(0)),
            window,
            cx,
        )
    });

    window.open_dialog(cx, move |dialog, _window, cx| {
        let model = model.clone();
        let search = search.clone();
        let level_select = level_select.clone();
        // Apply the current controls to the model before listing rows.
        let query = search.read(cx).value().to_string();
        let level = level_select
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        model.update(cx, |m, _| {
            m.text = query;
            m.level = level;
        });
        let rows: Vec<String> = model
            .read(cx)
            .visible()
            .iter()
            .map(|r| {
                format!(
                    "[{}] - {} - {} - at {}: {}",
                    r.timestamp, r.name, r.level, r.callsite, r.message
                )
            })
            .collect();
        dialog.title(tr("LogViewerDialog", "Logs")).child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .py_2()
                .w(gpui_kit::px(720.))
                .child(
                    div().flex().gap_2().child(
                        div()
                            .w(gpui_kit::px(160.))
                            .child(Select::new(&level_select)),
                    ),
                )
                .child(Input::new(&search).id("log-search"))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("log-refresh")
                                .label(tr("LogViewerDialog", "Refresh"))
                                .on_click({
                                    let model = model.clone();
                                    move |_, _, cx| {
                                        model.update(cx, |m, _| m.refresh());
                                    }
                                }),
                        )
                        .child(
                            Button::new("log-clear")
                                .label(tr("LogViewerDialog", "Clear Logs"))
                                .on_click({
                                    let model = model.clone();
                                    move |_, _, cx| {
                                        model.update(cx, |m, _| m.clear());
                                    }
                                }),
                        )
                        .child(
                            Button::new("log-open-dir")
                                .label(tr("LogViewerDialog", "Open Logs Directory"))
                                .on_click({
                                    let model = model.clone();
                                    move |_, _, cx| {
                                        let dir =
                                            model.read(cx).path.parent().map(|p| p.to_path_buf());
                                        if let Some(dir) = dir {
                                            let _ = open_logs_directory(&dir);
                                        }
                                    }
                                }),
                        ),
                )
                .child(
                    div()
                        .id("log-list")
                        .flex_1()
                        .min_h(gpui_kit::px(240.))
                        .max_h(gpui_kit::px(480.))
                        .overflow_y_scroll()
                        .children(rows.into_iter().map(|row| div().child(row))),
                ),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_strings_resolve_for_every_dialog() {
        // Source strings must exist in the generated catalog; tr()'s
        // source-fallback would silently mask typos.
        for (ctx, source) in [
            ("DuplicateDownloadDialog", "Duplicate Download Detected"),
            ("DuplicateDownloadDialog", "A duplicate download task already exists in the download list. Do you want to continue downloading?"),
            ("DuplicateDownloadDialog", "Task Name: {task_title}"),
            ("DuplicateDownloadDialog", "Don't ask again"),
            ("DuplicateDownloadDialog", "Continue"),
            ("DuplicateDownloadDialog", "Skip"),
            ("ExitDialog", "Close the main window"),
            ("ExitDialog", "Choose an action"),
            ("ExitDialog", "Exit the program"),
            ("ExitDialog", "Minimize to system tray"),
            ("LogViewerDialog", "Logs"),
            ("LogViewerDialog", "All"),
            ("LogViewerDialog", "Debug"),
            ("LogViewerDialog", "Info"),
            ("LogViewerDialog", "Warning"),
            ("LogViewerDialog", "Error"),
            ("LogViewerDialog", "Search logs..."),
            ("LogViewerDialog", "Refresh"),
            ("LogViewerDialog", "Clear Logs"),
            ("LogViewerDialog", "Open Logs Directory"),
        ] {
            let c = services::i18n::catalog(language());
            assert!(
                c.lookup(ctx, source).is_some(),
                "missing {ctx}/{source}"
            );
        }
    }

    #[test]
    fn tip_and_error_subsets_cover_row_states() {
        let c = services::i18n::catalog(language());
        for key in [
            "QUEUED",
            "PARSING",
            "PAUSED",
            "FFMPEG_QUEUED",
            "MERGING",
            "MERGING_WITH_PROGRESS",
            "CONVERTING",
            "CONVERTING_WITH_PROGRESS",
            "COMPLETED",
        ] {
            assert!(
                services::i18n::tip_messages(c, key).is_some(),
                "missing TIP {key}"
            );
        }
        for key in ["DOWNLOAD_FAILED", "FFMPEG_PROCESSING_FAILED"] {
            assert!(
                services::i18n::error_messages(c, key).is_some(),
                "missing ERROR {key}"
            );
        }
    }
}
