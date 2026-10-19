//! Select-area dialog (port of `src/gui/dialog/setting/select_area.py`):
//! picks the `Advanced.area` value (`"cn"` / `"ov"`).

use serde_json::json;

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "SelectAreaDialog";

/// Headless state: the selected area code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectAreaState {
    pub area: &'static str,
}

impl SelectAreaState {
    pub fn load(cfg: &config::ConfigStore) -> Self {
        Self {
            area: match cfg
                .get("Advanced", "area")
                .and_then(|v| v.as_str().map(str::to_owned))
                .as_deref()
            {
                Some("ov") => "ov",
                _ => "cn",
            },
        }
    }

    /// `accept()`: write `Advanced.area`, return the persisted value.
    pub fn apply(&self, cfg: &mut config::ConfigStore) -> &'static str {
        cfg.set("Advanced", "area", json!(self.area));
        self.area
    }
}

/// Open the dialog (Python `show_select_area_dialog`; also the
/// `run_tip_pass` fallback once `Misc.select_area_dialog_shown` is set).
pub fn open_select_area(cfg: config::ConfigStore, window: &mut Window, cx: &mut gpui_kit::App) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Select Geographic Location").to_owned();
    let desc = catalog
        .tr(
            CTX,
            "Please select your actual location. The program will automatically match a more suitable CDN server accordingly to improve download speed.",
        )
        .to_owned();
    let tip = catalog
        .tr(
            CTX,
            "Tip: If you are using a proxy, please select the region where the proxy server is located.",
        )
        .to_owned();
    let cn_label = catalog
        .tr(CTX, "China (Including Hong Kong, Macau, and Taiwan)")
        .to_owned();
    let ov_label = catalog.tr(CTX, "Other Regions").to_owned();
    let ok = "OK".to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(SelectAreaState::load(&cfg)));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));

    window.open_dialog(cx, move |dialog, _, _| {
        // `open_dialog` takes `Fn`; clone captures so inner `move`
        // closures own locals instead of this closure's captures.
        let (desc, tip, cn_label, ov_label) = (
            desc.clone(),
            tip.clone(),
            cn_label.clone(),
            ov_label.clone(),
        );
        let selected = if state.borrow().area == "ov" { 1 } else { 0 };
        let content_state = state.clone();
        let footer_state = state.clone();
        dialog
            .title(title.clone())
            .w(px(460.))
            .content(move |content, _, _| {
                content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Label::new(desc.clone()))
                        .child(Label::new(tip.clone()))
                        .child(
                            RadioGroup::new("area-choice")
                                .selected_index(Some(selected))
                                .child(cn_label.clone())
                                .child(ov_label.clone())
                                .on_change({
                                    let state = content_state.clone();
                                    move |ix, _, _| {
                                        state.borrow_mut().area =
                                            if *ix == 1 { "ov" } else { "cn" };
                                    }
                                }),
                        ),
                )
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = footer_state.clone();
                    let cfg_cell = cfg_cell.clone();
                    move |_, window, cx| {
                        state.borrow().apply(&mut cfg_cell.borrow_mut());
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
            "t31-select-area-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn loads_default_cn_and_persists_ov() {
        let mut cfg = scratch_cfg();
        assert_eq!(SelectAreaState::load(&cfg).area, "cn");

        let st = SelectAreaState { area: "ov" };
        assert_eq!(st.apply(&mut cfg), "ov");
        assert_eq!(
            cfg.get("Advanced", "area")
                .and_then(|v| v.as_str().map(str::to_owned))
                .as_deref(),
            Some("ov")
        );
        assert_eq!(SelectAreaState::load(&cfg).area, "ov");
    }

    #[test]
    fn translated_strings_resolve() {
        let c = services::i18n::catalog(services::i18n::ResolvedLanguage::ZhCn);
        assert_eq!(c.tr(CTX, "Other Regions"), "其他地区");
        assert_eq!(
            c.tr(
                "SelectAreaDialog",
                "China (Including Hong Kong, Macau, and Taiwan)"
            ),
            "中国（含港澳台地区）"
        );
    }
}
