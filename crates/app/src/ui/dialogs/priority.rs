//! Priority dialog (port of `src/gui/dialog/setting/priority.py`):
//! reorders the `Download.{video_quality,audio_quality,video_codec}_priority`
//! arrays. Python uses a drag list; gpui-kit has none, so each row gets
//! ↑/↓ buttons mutating the same backing `Vec<i64>`.

use serde_json::{json, Value};

use gpui_kit::base::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "PriorityDialog";

/// Which priority list the dialog edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityKind {
    VideoQuality,
    AudioQuality,
    VideoCodec,
}

impl PriorityKind {
    /// The `Download` config key (`on_adjust_*_priority` in setting.py).
    pub fn key(self) -> &'static str {
        match self {
            Self::VideoQuality => "video_quality_priority",
            Self::AudioQuality => "audio_quality_priority",
            Self::VideoCodec => "video_codec_priority",
        }
    }

    /// Localized row label for one id (the reversed `Translator.*` map in
    /// setting.py; codecs keep the raw key).
    pub fn label(self, id: i64, c: &services::i18n::Catalog) -> String {
        type Tr = fn(&services::i18n::Catalog, &str) -> Option<String>;
        let (key, tr): (&str, Tr) = match self {
            Self::VideoQuality => (
                parsers::playurl::video_quality_label(id).unwrap_or(""),
                services::i18n::video_quality,
            ),
            Self::AudioQuality => (
                parsers::playurl::audio_quality_label(id).unwrap_or(""),
                services::i18n::audio_quality,
            ),
            Self::VideoCodec => (
                parsers::playurl::codec_label(id).unwrap_or(""),
                services::i18n::video_codec,
            ),
        };
        tr(c, key).unwrap_or_else(|| key.to_owned())
    }
}

/// Headless state: the working order (ids in priority order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityState {
    pub order: Vec<i64>,
}

impl PriorityState {
    pub fn load(cfg: &config::ConfigStore, kind: PriorityKind) -> Self {
        Self {
            order: cfg
                .get("Download", kind.key())
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default(),
        }
    }

    /// Swap row `ix` with its neighbor (`up` = towards higher priority).
    /// Python's drag list constraint: row 0 stays reachable only by
    /// moving others, so the boundary swaps are the no-op guards.
    pub fn move_row(&mut self, ix: usize, up: bool) {
        let target = if up { ix.checked_sub(1) } else { Some(ix + 1) };
        if let Some(target) = target {
            if target < self.order.len() {
                self.order.swap(ix, target);
            }
        }
    }

    /// `accept()` → `config.set(config.<kind>_priority, …)`.
    pub fn apply(&self, cfg: &mut config::ConfigStore, kind: PriorityKind) {
        cfg.set("Download", kind.key(), json!(self.order));
    }
}

/// Open the priority dialog for one kind.
pub fn open_priority(
    cfg: config::ConfigStore,
    kind: PriorityKind,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Customize Priority").to_owned();
    let tip = catalog
        .tr(
            CTX,
            "Drag items to reorder. Higher items have higher priority.",
        )
        .to_owned();
    let ok = "OK".to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(PriorityState::load(&cfg, kind)));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));
    let order_cell = state.clone();

    window.open_dialog(cx, move |dialog, _, _| {
        // `open_dialog` takes `Fn`; clone captures so inner `move`
        // closures own locals instead of this closure's captures.
        let tip = tip.clone();
        let rows = {
            let state = state.borrow();
            state
                .order
                .iter()
                .enumerate()
                .map(|(ix, &id)| (ix, kind.label(id, catalog)))
                .collect::<Vec<_>>()
        };
        let content_rows = rows.clone();
        let content_order_cell = order_cell.clone();
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
                        .child(Label::new(tip.clone()))
                        .child(
                            div()
                                .id("priority-rows")
                                .flex()
                                .flex_col()
                                .gap_1()
                                .max_h(px(360.))
                                .overflow_y_scroll()
                                .children(content_rows.iter().map(|&(ix, ref label)| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(div().flex_1().child(Label::new(label)))
                                        .child(
                                            Button::new(("priority-up", ix))
                                                .label("↑")
                                                .outline()
                                                .disabled(ix == 0)
                                                .on_click({
                                                    let order_cell = content_order_cell.clone();
                                                    move |_, _, cx| {
                                                        order_cell.borrow_mut().move_row(ix, true);
                                                        cx.refresh_windows();
                                                    }
                                                }),
                                        )
                                        .child(
                                            Button::new(("priority-down", ix))
                                                .label("↓")
                                                .outline()
                                                .disabled(
                                                    ix + 1
                                                        >= content_order_cell.borrow().order.len(),
                                                )
                                                .on_click({
                                                    let order_cell = content_order_cell.clone();
                                                    move |_, _, cx| {
                                                        order_cell.borrow_mut().move_row(ix, false);
                                                        cx.refresh_windows();
                                                    }
                                                }),
                                        )
                                })),
                        ),
                )
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = footer_state.clone();
                    let cfg_cell = cfg_cell.clone();
                    move |_, window, cx| {
                        state.borrow().apply(&mut cfg_cell.borrow_mut(), kind);
                        if let Err(e) = cfg_cell.borrow().save() {
                            window.push_notification(Notification::error(e.to_string()), cx);
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
            "t31-priority-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn keys_exist_and_round_trip() {
        let mut cfg = scratch_cfg();
        for kind in [
            PriorityKind::VideoQuality,
            PriorityKind::AudioQuality,
            PriorityKind::VideoCodec,
        ] {
            // defaults.json must define the key, else `set` drops it.
            assert!(
                cfg.get("Download", kind.key()).is_some(),
                "missing {}",
                kind.key()
            );
            let mut st = PriorityState::load(&cfg, kind);
            assert!(!st.order.is_empty());
            st.move_row(0, false); // swap first two
            st.apply(&mut cfg, kind);
            assert_eq!(PriorityState::load(&cfg, kind), st);
        }
    }

    #[test]
    fn move_row_guards_boundaries() {
        let mut st = PriorityState {
            order: vec![7, 12, 13],
        };
        st.move_row(0, true); // no-op at top
        assert_eq!(st.order, vec![7, 12, 13]);
        st.move_row(2, false); // no-op at bottom
        assert_eq!(st.order, vec![7, 12, 13]);
        st.move_row(1, true);
        assert_eq!(st.order, vec![12, 7, 13]);
        st.move_row(1, false);
        assert_eq!(st.order, vec![12, 13, 7]);
    }

    #[test]
    fn labels_localize() {
        let c = services::i18n::catalog(services::i18n::ResolvedLanguage::ZhCn);
        assert_eq!(PriorityKind::VideoQuality.label(127, c), "8K 超高清");
        assert_eq!(PriorityKind::AudioQuality.label(30251, c), "Hi-Res 无损");
        assert_eq!(PriorityKind::VideoCodec.label(13, c), "AV1");
    }
}
