//! Auto-select dialog (port of
//! `src/gui/dialog/setting/auto_select.py`): mode radio plus the three
//! per-category conditions, written to `Behavior.auto_select_mode_`
//! (0 manual / 1 select-all / 2 conditional) and
//! `Behavior.auto_select_conditions` — the exact ints
//! `ParseViewModel::apply_auto_select` reads.

use serde_json::{json, Map, Value};

use gpui_kit::base::IndexPath;
use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "AutoSelectDialog";

/// Headless state: mode int + the three condition ints (0/1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoSelectState {
    pub mode: i64,
    pub user_uploads: i64,
    pub bangumi: i64,
    pub other: i64,
}

impl AutoSelectState {
    pub fn load(cfg: &config::ConfigStore) -> Self {
        let cond = |key: &str| {
            cfg.get("Behavior", "auto_select_conditions")
                .and_then(|v| v.get(key))
                .and_then(Value::as_i64)
                .unwrap_or(0)
        };
        Self {
            mode: cfg
                .get("Behavior", "auto_select_mode_")
                .and_then(Value::as_i64)
                .unwrap_or(2),
            user_uploads: cond("user_uploads"),
            bangumi: cond("bangumi"),
            other: cond("other"),
        }
    }

    /// `accept()`: write the mode and the conditions map.
    pub fn apply(&self, cfg: &mut config::ConfigStore) {
        cfg.set("Behavior", "auto_select_mode_", json!(self.mode));
        let mut m = Map::new();
        m.insert("user_uploads".into(), json!(self.user_uploads));
        m.insert("bangumi".into(), json!(self.bangumi));
        m.insert("other".into(), json!(self.other));
        cfg.set("Behavior", "auto_select_conditions", Value::Object(m));
    }
}

/// Open the dialog.
pub fn open_auto_select(cfg: config::ConfigStore, window: &mut Window, cx: &mut gpui_kit::App) {
    let catalog = active_catalog(&cfg);
    let title = catalog
        .tr(CTX, "Auto-select Download Items Settings")
        .to_owned();
    let desc = catalog
        .tr(
            CTX,
            "Set how items in the parsed list are automatically selected after parsing is complete.",
        )
        .to_owned();
    let mode_manual = catalog.tr(CTX, "Let me choose manually").to_owned();
    let mode_all = catalog.tr(CTX, "Automatically select all items").to_owned();
    let mode_cond = catalog
        .tr(CTX, "Automatically select based on conditions")
        .to_owned();
    let hint = catalog
        .tr(
            CTX,
            "The following rules can only be modified when \"Automatically select based on conditions\" is selected.",
        )
        .to_owned();
    let user_label = catalog
        .tr(CTX, "When parsing user-uploaded videos")
        .to_owned();
    let user_opts = [
        catalog
            .tr(
                CTX,
                "Select only the single video corresponding to the link",
            )
            .to_owned(),
        catalog
            .tr(CTX, "Select all videos in the multi-part or collection")
            .to_owned(),
    ];
    let bangumi_label = catalog
        .tr(CTX, "When parsing episodic or course-type videos")
        .to_owned();
    let bangumi_opts = [
        catalog
            .tr(
                CTX,
                "Select only the single episode corresponding to the link",
            )
            .to_owned(),
        catalog
            .tr(CTX, "Select all main episodes in the series")
            .to_owned(),
    ];
    let other_label = catalog
        .tr(CTX, "When parsing other types of videos")
        .to_owned();
    let other_opts = [
        catalog.tr(CTX, "Choose manually").to_owned(),
        catalog.tr(CTX, "Automatically select all items").to_owned(),
    ];
    let ok = "OK".to_owned();

    let st = AutoSelectState::load(&cfg);
    let state = std::rc::Rc::new(std::cell::RefCell::new(st));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));

    let mut mk_select = |items: [String; 2], selected: i64| {
        let ix = usize::from(selected == 1);
        let items: Vec<String> = items.into();
        cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(items),
                Some(IndexPath::new(ix)),
                window,
                cx,
            )
        })
    };
    let user_sel = mk_select(user_opts, st.user_uploads);
    let bangumi_sel = mk_select(bangumi_opts, st.bangumi);
    let other_sel = mk_select(other_opts, st.other);

    window.open_dialog(cx, move |dialog, _, _| {
        // `open_dialog` takes `Fn`; clone captures so inner `move`
        // closures own locals instead of this closure's captures.
        let (desc, hint, mode_manual, mode_all, mode_cond) = (
            desc.clone(),
            hint.clone(),
            mode_manual.clone(),
            mode_all.clone(),
            mode_cond.clone(),
        );
        let (user_label, bangumi_label, other_label) = (
            user_label.clone(),
            bangumi_label.clone(),
            other_label.clone(),
        );
        let (user_sel, bangumi_sel, other_sel) =
            (user_sel.clone(), bangumi_sel.clone(), other_sel.clone());
        let st = *state.borrow();
        let selected = match st.mode {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        dialog
            .title(title.clone())
            .w(px(520.))
            .content({
                let state = state.clone();
                let user_sel = user_sel.clone();
                let bangumi_sel = bangumi_sel.clone();
                let other_sel = other_sel.clone();
                move |content, _, _cx| {
                    let st = *state.borrow();
                    let conditional = st.mode == 2;
                    content.child(
                        div()
                            .id("auto-select-body")
                            .flex()
                            .flex_col()
                            .gap_2()
                            .max_h(px(420.))
                            .overflow_y_scroll()
                            .child(Label::new(desc.clone()))
                            .child(
                                RadioGroup::new("auto-select-mode")
                                    .selected_index(Some(selected))
                                    .child(mode_manual.clone())
                                    .child(mode_all.clone())
                                    .child(mode_cond.clone())
                                    .on_change({
                                        let state = state.clone();
                                        move |ix, _, _| {
                                            state.borrow_mut().mode = *ix as i64;
                                        }
                                    }),
                            )
                            .child(Label::new(hint.clone()))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .when(!conditional, |this| this.opacity(0.5))
                                    .child(Label::new(user_label.clone()))
                                    .child(Select::new(&user_sel).disabled(!conditional))
                                    .child(Label::new(bangumi_label.clone()))
                                    .child(Select::new(&bangumi_sel).disabled(!conditional))
                                    .child(Label::new(other_label.clone()))
                                    .child(Select::new(&other_sel).disabled(!conditional)),
                            ),
                    )
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = state.clone();
                    let cfg_cell = cfg_cell.clone();
                    let user_sel = user_sel.clone();
                    let bangumi_sel = bangumi_sel.clone();
                    let other_sel = other_sel.clone();
                    move |_, window, cx| {
                        let mut st = *state.borrow();
                        st.user_uploads = user_sel
                            .read(cx)
                            .selected_index(cx)
                            .map(|p| p.row as i64)
                            .unwrap_or(0);
                        st.bangumi = bangumi_sel
                            .read(cx)
                            .selected_index(cx)
                            .map(|p| p.row as i64)
                            .unwrap_or(0);
                        st.other = other_sel
                            .read(cx)
                            .selected_index(cx)
                            .map(|p| p.row as i64)
                            .unwrap_or(0);
                        st.apply(&mut cfg_cell.borrow_mut());
                        if let Err(e) = cfg_cell.borrow().save() {
                            window.push_notification(
                                gpui_kit::component::notification::Notification::error(
                                    e.to_string(),
                                ),
                                cx,
                            );
                            return;
                        }
                        window.close_dialog(cx);
                    }
                })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_cfg() -> config::ConfigStore {
        let dir = std::env::temp_dir().join(format!(
            "t31-auto-select-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn round_trips_mode_and_conditions() {
        let mut cfg = scratch_cfg();
        assert_eq!(AutoSelectState::load(&cfg).mode, 2); // default CONDITIONAL

        let st = AutoSelectState {
            mode: 1,
            user_uploads: 0,
            bangumi: 1,
            other: 1,
        };
        st.apply(&mut cfg);
        assert_eq!(
            cfg.get("Behavior", "auto_select_mode_")
                .and_then(Value::as_i64),
            Some(1)
        );
        assert_eq!(AutoSelectState::load(&cfg), st);
    }
}
