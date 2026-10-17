//! Interactive-video dialog (port of
//! `src/gui/dialog/misc/interactive_video.py`).
//!
//! Offers "Automatically parse all nodes" (the only option Python
//! ships; the manual path is commented out upstream). Accept returns
//! the payload the walk needs; the stein-graph traversal itself is
//! `parsers::media_parser::parse_interactive`.
use serde_json::Value;

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

/// i18n context of the Python dialog class.
const CTX: &str = "InteractiveVideoDialog";

/// Headless payload of the dialog (Python `AutoParsePayload` with
/// `parser_type=INTERACTIVE_VIDEO`).
#[derive(Debug, Clone)]
pub struct InteractivePayload {
    /// The view `data` of the stein-gated video (entry node info).
    pub data: Value,
    /// "Automatically parse all nodes" radio (default and only true
    /// value upstream).
    pub auto_parse: bool,
}

#[derive(Debug, Clone, Default)]
pub struct InteractiveState {
    pub auto_parse: bool,
}

impl InteractiveState {
    pub fn new() -> Self {
        Self { auto_parse: true }
    }

    /// `get_data()` — the payload accepted with the dialog.
    pub fn payload(&self, data: Value) -> InteractivePayload {
        InteractivePayload {
            data,
            auto_parse: self.auto_parse,
        }
    }
}

/// Localized tip line ("Detection of interactive video, ...").
pub fn tip(c: &services::i18n::Catalog) -> String {
    c.tr(
        CTX,
        "Detection of interactive video, please select an operation",
    )
    .to_owned()
}

/// Localized radio label.
pub fn auto_label(c: &services::i18n::Catalog) -> String {
    c.tr(CTX, "Automatically parse all nodes").to_owned()
}

/// Open the dialog. `on_accept` receives the payload; the walk
/// (auto-parse of every stein node) runs in the parse screen.
pub fn open_interactive_video(
    data: Value,
    cfg: config::ConfigStore,
    on_accept: impl FnOnce(InteractivePayload, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let data = std::rc::Rc::new(data);
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Interactive Video").to_owned();
    let tip = tip(catalog);
    let label = auto_label(catalog);
    let ok = "OK".to_owned();
    let state = std::rc::Rc::new(std::cell::RefCell::new(InteractiveState::new()));
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(title.clone())
            .w(px(480.))
            .content({
                let state = state.clone();
                let tip = tip.clone();
                let label = label.clone();
                move |content, _, _| {
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(Label::new(tip.clone()))
                            .child(
                                RadioGroup::new("interactive")
                                    .selected_index(Some(0))
                                    .child(
                                        Radio::new("auto")
                                            .checked(true)
                                            .label(label.clone())
                                            .on_click({
                                                let state = state.clone();
                                                move |checked, _, _| {
                                                    state.borrow_mut().auto_parse = *checked;
                                                }
                                            }),
                                    ),
                            ),
                    )
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = state.clone();
                    let on_accept = on_accept.clone();
                    let data = std::rc::Rc::clone(&data);
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        let st = state.borrow_mut();
                        if let Some(cb) = on_accept.borrow_mut().take() {
                            cb(st.payload((*data).clone()), cx);
                        }
                    }
                })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use services::i18n::{catalog, ResolvedLanguage};

    #[test]
    fn default_offers_auto_parse_only() {
        let st = InteractiveState::new();
        assert!(st.auto_parse);
        let p = st.payload(serde_json::json!({"bvid": "BV1xx"}));
        assert!(p.auto_parse);
        assert_eq!(p.data["bvid"], "BV1xx");
    }

    #[test]
    fn tip_translates() {
        let c = catalog(ResolvedLanguage::ZhCn);
        assert_eq!(tip(c), "检测到互动视频，请选择操作");
        assert_eq!(auto_label(c), "自动解析所有节点");
    }
}
