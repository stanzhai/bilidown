//! Danmaku / subtitle style dialogs (ports of
//! `src/gui/dialog/setting/{danmaku_style,subtitles_style}.py` +
//! the setting groups in `gui/component/setting/group.py`).
//!
//! Both edit their `Additional.danmaku_style` /
//! `Additional.subtitle_style` config section as plain JSON maps with
//! exactly the Python dict keys — that is the format the config file
//! round-trips and `extras::ass::SubtitleStyle::from_config` reads.
//!
//! Note: the subtitle `alignment` key is lowercase here (the dialog's
//! `AlignmentGroup` writes `"alignment"`), while
//! `SubtitleStyle::from_config` still reads a capitalized
//! `"Alignment"` — `subtitle_style_value` writes both so the ASS
//! renderer sees the edit until that reader is aligned with Python.

use serde_json::{json, Map, Value};

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, AppContext, ParentElement, Window};

use super::active_catalog;

pub const SUBTITLES_ALIGNMENT_MAP: [(&str, i64); 9] = [
    ("BOTTOM_LEFT", 1),
    ("BOTTOM_CENTER", 2),
    ("BOTTOM_RIGHT", 3),
    ("MIDDLE_LEFT", 4),
    ("MIDDLE_CENTER", 5),
    ("MIDDLE_RIGHT", 6),
    ("TOP_LEFT", 7),
    ("TOP_CENTER", 8),
    ("TOP_RIGHT", 9),
];

/// Localized alignment label (`"Bottom Left (2)"` shape).
pub fn alignment_label(c: &services::i18n::Catalog, key: &str, value: i64) -> String {
    format!(
        "{} ({})",
        services::i18n::subtitles_alignment(c, key).unwrap_or_else(|| key.to_owned()),
        value
    )
}
/// The current alignment row label ("Bottom Center (2)" localized).
pub fn alignment_caption(c: &services::i18n::Catalog, value: i64) -> String {
    SUBTITLES_ALIGNMENT_MAP
        .iter()
        .find(|(_, v)| *v == value)
        .map(|(k, v)| alignment_label(c, k, *v))
        .unwrap_or_else(|| c.tr("AlignmentGroup", "Alignment").to_owned())
}

// Danmaku style
// ---------------------------------------------------------------------

/// Headless view-model of the danmaku style dialog: one group map per
/// Python setting group.
#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuStyleState {
    pub font: Map<String, Value>,
    pub border: Map<String, Value>,
    pub advanced: Map<String, Value>,
    pub resolution: Map<String, Value>,
}

impl DanmakuStyleState {
    /// `init_data`: read the section, defaulting per group.
    pub fn load(cfg: &config::ConfigStore) -> Self {
        let section = cfg.get("Additional", "danmaku_style").cloned();
        let g = |name: &str| {
            section
                .as_ref()
                .and_then(|v| v.get(name))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
        };
        let mut font = g("font");
        let mut border = g("border");
        let mut advanced = g("advanced");
        let mut resolution = g("resolution");

        let ensure = |map: &mut Map<String, Value>, k: &str, v: Value| {
            map.entry(k.to_owned()).or_insert(v);
        };
        ensure(&mut font, "name", json!("黑体"));
        ensure(&mut font, "size", json!(36));
        for k in ["bold", "italic", "underline", "strike"] {
            ensure(&mut font, k, json!(false));
        }
        ensure(&mut border, "border", json!(1.0));
        ensure(&mut border, "shadow", json!(0));
        ensure(&mut advanced, "display_area", json!(60));
        ensure(&mut advanced, "opacity", json!(80));
        ensure(&mut advanced, "scroll_duration", json!(10));
        ensure(&mut advanced, "static_duration", json!(5));
        ensure(&mut advanced, "minimum_gap", json!(100));
        ensure(&mut resolution, "width", json!(1280));
        ensure(&mut resolution, "height", json!(720));

        Self {
            font,
            border,
            advanced,
            resolution,
        }
    }

    /// `accept()` → the dict written back to
    /// `Additional.danmaku_style` (then `ConfigStore::set` + `save`).
    pub fn to_value(&self) -> Value {
        json!({
            "font": self.font,
            "border": self.border,
            "advanced": self.advanced,
            "resolution": self.resolution,
        })
    }

    pub fn save(&self, cfg: &mut config::ConfigStore) -> anyhow::Result<()> {
        cfg.set("Additional", "danmaku_style", self.to_value());
        cfg.save()
    }
}

// ---------------------------------------------------------------------
// Subtitle style
// ---------------------------------------------------------------------

/// Headless view-model of the subtitle style dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleStyleState {
    pub font: Map<String, Value>,
    pub border: Map<String, Value>,
    pub color: Map<String, Value>,
    pub margin: Map<String, Value>,
    pub resolution: Map<String, Value>,
    /// Keypad alignment 1–9.
    pub alignment: i64,
}

impl SubtitleStyleState {
    pub fn load(cfg: &config::ConfigStore) -> Self {
        let section = cfg.get("Additional", "subtitle_style").cloned();
        let g = |name: &str| {
            section
                .as_ref()
                .and_then(|v| v.get(name))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
        };
        let mut font = g("font");
        let mut border = g("border");
        let mut color = g("color");
        let mut margin = g("margin");
        let mut resolution = g("resolution");

        let ensure = |map: &mut Map<String, Value>, k: &str, v: Value| {
            map.entry(k.to_owned()).or_insert(v);
        };
        ensure(&mut font, "name", json!("黑体"));
        ensure(&mut font, "size", json!(36));
        for k in ["bold", "italic", "underline", "strike"] {
            ensure(&mut font, k, json!(false));
        }
        ensure(&mut border, "border", json!(1.0));
        ensure(&mut border, "shadow", json!(0.0));
        ensure(&mut color, "primary", json!("&H00FFFFFF"));
        ensure(&mut color, "secondary", json!("&H000000FF"));
        ensure(&mut color, "border", json!("H00000000"));
        ensure(&mut color, "shadow", json!("H00000000"));
        ensure(&mut margin, "left", json!(10));
        ensure(&mut margin, "right", json!(10));
        ensure(&mut margin, "vertical", json!(20));
        ensure(&mut resolution, "width", json!(1280));
        ensure(&mut resolution, "height", json!(720));

        // Python reads `alignment` (lowercase); the Rust ASS reader
        // still reads `Alignment` — accept either.
        let alignment = section
            .as_ref()
            .and_then(|v| {
                v.get("alignment")
                    .or_else(|| v.get("Alignment"))
                    .and_then(Value::as_i64)
            })
            .unwrap_or(2);

        Self {
            font,
            border,
            color,
            margin,
            resolution,
            alignment,
        }
    }

    /// `accept()` → the dict written back to `Additional.subtitle_style`.
    pub fn to_value(&self) -> Value {
        json!({
            "font": self.font,
            "border": self.border,
            "color": self.color,
            "margin": self.margin,
            "resolution": self.resolution,
            "alignment": self.alignment,
            // ponytail: duplicated key for extras::ass::from_config,
            // which reads "Alignment"; drop when that reader matches
            // Python's lowercase key.
            "Alignment": self.alignment,
        })
    }

    pub fn save(&self, cfg: &mut config::ConfigStore) -> anyhow::Result<()> {
        cfg.set("Additional", "subtitle_style", self.to_value());
        cfg.save()
    }
}

/// Localized dialog captions.
pub fn danmaku_caption(c: &services::i18n::Catalog) -> String {
    c.tr("ContentWidget", "Customize Danmaku Style").to_owned()
}

pub fn subtitle_caption(c: &services::i18n::Catalog) -> String {
    c.tr("ContentWidget", "Customize Subtitle Style").to_owned()
}

/// One editable row: a text field bound to a map entry (string-typed
/// in the UI; parsed back to the field's JSON type on save).
struct StyleField {
    input: gpui_kit::Entity<InputState>,
    group: &'static str,
    key: &'static str,
    label: String,
    /// `i` int / `f` float / `b` bool / `s` string.
    kind: char,
}

/// Fold the dialog rows back into `(group, key, value)` writes.
fn collect_fields(
    fields: &[StyleField],
    cx: &gpui_kit::App,
) -> Result<Vec<(&'static str, &'static str, Value)>, String> {
    let mut out = Vec::new();
    for field in fields {
        let text = field.input.read(cx).value().to_string();
        let text = text.trim().to_owned();
        let value = match field.kind {
            'i' => text
                .parse::<i64>()
                .map_err(|_| format!("{}: {}", field.label, text))?
                .into(),
            'f' => text
                .parse::<f64>()
                .map_err(|_| format!("{}: {}", field.label, text))?
                .into(),
            'b' => Value::Bool(text == "true" || text == "1"),
            _ => Value::String(text),
        };
        out.push((field.group, field.key, value));
    }
    Ok(out)
}

/// Build the dialog rows; inputs are pre-filled from the loaded maps.
fn style_fields(
    window: &mut Window,
    cx: &mut gpui_kit::App,
    rows: Vec<(&'static str, &'static str, String, char)>,
    read: &dyn Fn(&str, &str) -> Value,
) -> Vec<StyleField> {
    rows.into_iter()
        .map(|(group, key, label, kind)| {
            let text = match read(group, key) {
                Value::String(s) => s,
                v => v.to_string(),
            };
            let input = cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder(&label);
                s.set_value(text, window, cx);
                s
            });
            StyleField {
                input,
                group,
                key,
                label,
                kind,
            }
        })
        .collect()
}

/// Open the danmaku-style dialog (`ContentWidget` →
/// `Additional.danmaku_style`).
pub fn open_danmaku_style(cfg: config::ConfigStore, window: &mut Window, cx: &mut gpui_kit::App) {
    let catalog = active_catalog(&cfg);
    let state = DanmakuStyleState::load(&cfg);
    let caption = danmaku_caption(catalog);
    let ok = catalog.tr("RuleListDialog", "Save").to_owned();
    let labels = |ctx: &str, src: &str| catalog.tr(ctx, src).to_owned();

    let maps = (
        state.font.clone(),
        state.border.clone(),
        state.advanced.clone(),
        state.resolution.clone(),
    );
    let read = move |group: &str, key: &str| -> Value {
        let (font, border, advanced, resolution) = &maps;
        let map = match group {
            "font" => font,
            "border" => border,
            "advanced" => advanced,
            _ => resolution,
        };
        map.get(key).cloned().unwrap_or(Value::Null)
    };

    let rows: Vec<(&'static str, &'static str, String, char)> = vec![
        ("font", "name", labels("FontGroup", "Font Family"), 's'),
        ("font", "size", labels("FontGroup", "Font Size"), 'i'),
        ("font", "bold", labels("FontGroup", "Bold"), 'b'),
        ("font", "italic", labels("FontGroup", "Italic"), 'b'),
        ("font", "underline", labels("FontGroup", "Underline"), 'b'),
        ("font", "strike", labels("FontGroup", "Strikethrough"), 'b'),
        (
            "border",
            "border",
            labels("BorderGroup", "Outline (px)"),
            'f',
        ),
        (
            "border",
            "shadow",
            labels("BorderGroup", "Shadow (px)"),
            'f',
        ),
        (
            "advanced",
            "display_area",
            labels("AdvancedGroup", "Display Area (%)"),
            'i',
        ),
        (
            "advanced",
            "opacity",
            labels("AdvancedGroup", "Opacity (%)"),
            'i',
        ),
        (
            "advanced",
            "scroll_duration",
            labels("AdvancedGroup", "Scroll Duration (s)"),
            'i',
        ),
        (
            "advanced",
            "static_duration",
            labels("AdvancedGroup", "Static Duration (s)"),
            'i',
        ),
        (
            "advanced",
            "minimum_gap",
            labels("AdvancedGroup", "Minimum Horizontal Spacing (px)"),
            'i',
        ),
        (
            "resolution",
            "width",
            labels("ResolutionGroup", "Screen Width (px)"),
            'i',
        ),
        (
            "resolution",
            "height",
            labels("ResolutionGroup", "Screen Height (px)"),
            'i',
        ),
    ];
    let fields = style_fields(window, cx, rows, &read);
    let fields = std::rc::Rc::new(fields);
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(caption.clone())
            .w(px(460.))
            .content({
                let fields = fields.clone();
                move |content, _, _| {
                    content.child(div().flex().flex_col().gap_2().children(fields.iter().map(
                        |f| {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(180.)).child(Label::new(f.label.clone())))
                                .child(div().flex_1().child(Input::new(&f.input).id(f.key)))
                        },
                    )))
                }
            })
            .footer({
                let fields = fields.clone();
                let cfg_cell = cfg_cell.clone();
                DialogFooter::new().child(Button::new("save").label(ok.clone()).on_click(
                    move |_, window, cx| {
                        let fields = fields.clone();
                        let writes = match collect_fields(&fields, cx) {
                            Ok(w) => w,
                            Err(e) => {
                                window.push_notification(Notification::error(e.clone()), cx);
                                return;
                            }
                        };
                        let mut st = DanmakuStyleState::load(&cfg_cell.borrow());
                        for (group, key, value) in writes {
                            let map = match group {
                                "font" => &mut st.font,
                                "border" => &mut st.border,
                                "advanced" => &mut st.advanced,
                                _ => &mut st.resolution,
                            };
                            map.insert(key.to_owned(), value);
                        }
                        if let Err(e) = st.save(&mut cfg_cell.borrow_mut()) {
                            window.push_notification(Notification::error(e.to_string()), cx);
                            return;
                        }
                        window.close_dialog(cx);
                    },
                ))
            })
    });
}

/// Open the subtitle-style dialog (`ContentWidget` →
/// `Additional.subtitle_style`).
pub fn open_subtitle_style(cfg: config::ConfigStore, window: &mut Window, cx: &mut gpui_kit::App) {
    let catalog = active_catalog(&cfg);
    let state = SubtitleStyleState::load(&cfg);
    let caption = subtitle_caption(catalog);
    let ok = catalog.tr("RuleListDialog", "Save").to_owned();
    let labels = |ctx: &str, src: &str| catalog.tr(ctx, src).to_owned();

    let maps = (
        state.font.clone(),
        state.border.clone(),
        state.color.clone(),
        state.margin.clone(),
        state.resolution.clone(),
    );
    let read = move |group: &str, key: &str| -> Value {
        let (font, border, color, margin, resolution) = &maps;
        let map = match group {
            "font" => font,
            "border" => border,
            "color" => color,
            "margin" => margin,
            _ => resolution,
        };
        map.get(key).cloned().unwrap_or(Value::Null)
    };

    let rows: Vec<(&'static str, &'static str, String, char)> = vec![
        ("font", "name", labels("FontGroup", "Font Family"), 's'),
        ("font", "size", labels("FontGroup", "Font Size"), 'i'),
        ("font", "bold", labels("FontGroup", "Bold"), 'b'),
        (
            "border",
            "outline",
            labels("BorderGroup", "Outline (px)"),
            'f',
        ),
        (
            "border",
            "shadow",
            labels("BorderGroup", "Shadow (px)"),
            'f',
        ),
        (
            "color",
            "primary",
            labels("ColorGroup", "Primary Color"),
            's',
        ),
        (
            "color",
            "secondary",
            labels("ColorGroup", "Secondary Color"),
            's',
        ),
        (
            "color",
            "outline",
            labels("ColorGroup", "Outline Color"),
            's',
        ),
        ("color", "back", labels("ColorGroup", "Back Color"), 's'),
        (
            "margin",
            "vertical",
            labels("MarginGroup", "Vertical Margin (px)"),
            'i',
        ),
        (
            "margin",
            "horizontal",
            labels("MarginGroup", "Horizontal Margin (px)"),
            'i',
        ),
        (
            "resolution",
            "width",
            labels("ResolutionGroup", "Screen Width (px)"),
            'i',
        ),
        (
            "resolution",
            "height",
            labels("ResolutionGroup", "Screen Height (px)"),
            'i',
        ),
        (
            "",
            "alignment",
            alignment_caption(catalog, state.alignment),
            'i',
        ),
    ];
    let fields = std::rc::Rc::new(style_fields(window, cx, rows, &read));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(caption.clone())
            .w(px(460.))
            .content({
                let fields = fields.clone();
                move |content, _, _| {
                    content.child(div().flex().flex_col().gap_2().children(fields.iter().map(
                        |f| {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(180.)).child(Label::new(f.label.clone())))
                                .child(div().flex_1().child(Input::new(&f.input).id(f.key)))
                        },
                    )))
                }
            })
            .footer({
                let fields = fields.clone();
                let cfg_cell = cfg_cell.clone();
                DialogFooter::new().child(Button::new("save").label(ok.clone()).on_click(
                    move |_, window, cx| {
                        let fields = fields.clone();
                        let writes = match collect_fields(&fields, cx) {
                            Ok(w) => w,
                            Err(e) => {
                                window.push_notification(Notification::error(e.clone()), cx);
                                return;
                            }
                        };
                        let mut st = SubtitleStyleState::load(&cfg_cell.borrow());
                        for (group, key, value) in writes {
                            if group.is_empty() {
                                st.alignment = value.as_i64().unwrap_or(st.alignment);
                                continue;
                            }
                            let map = match group {
                                "font" => &mut st.font,
                                "border" => &mut st.border,
                                "color" => &mut st.color,
                                "margin" => &mut st.margin,
                                _ => &mut st.resolution,
                            };
                            map.insert(key.to_owned(), value);
                        }
                        if let Err(e) = st.save(&mut cfg_cell.borrow_mut()) {
                            window.push_notification(Notification::error(e.to_string()), cx);
                            return;
                        }
                        window.close_dialog(cx);
                    },
                ))
            })
    });
}

// ---------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use services::i18n::{catalog, ResolvedLanguage};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique throwaway config dir per test (round-trips must not
    /// leak into other tests' `config.json`).
    fn scratch_cfg() -> config::ConfigStore {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t25-styles-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn danmaku_load_defaults_match_the_config_fixture() {
        let cfg = scratch_cfg();
        let st = DanmakuStyleState::load(&cfg);
        assert_eq!(st.font["name"], "黑体");
        assert_eq!(st.font["size"], 36);
        assert_eq!(st.border["border"], 1.0);
        assert_eq!(st.advanced["display_area"], 60);
        assert_eq!(st.advanced["minimum_gap"], 100);
        assert_eq!(st.resolution["width"], 1280);
        assert_eq!(st.resolution["height"], 720);
    }

    #[test]
    fn danmaku_round_trips_through_the_config_section() {
        let mut cfg = scratch_cfg();
        let mut st = DanmakuStyleState::load(&cfg);
        st.font.insert("name".into(), json!("楷体"));
        st.font.insert("size".into(), json!(48));
        st.font.insert("bold".into(), json!(true));
        st.border.insert("shadow".into(), json!(2.5));
        st.advanced.insert("opacity".into(), json!(90));
        st.resolution.insert("width".into(), json!(1920));

        st.save(&mut cfg).unwrap();

        let reread = DanmakuStyleState::load(&cfg);
        assert_eq!(reread, st);
        let section = cfg.get("Additional", "danmaku_style").unwrap();
        assert_eq!(section["font"]["name"], "楷体");
        assert_eq!(section["font"]["size"], 48);
        assert_eq!(section["advanced"]["opacity"], 90);
        // exactly the Python groups, no extras
        let keys: Vec<_> = section.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["font", "border", "advanced", "resolution"]);
    }

    #[test]
    fn subtitle_load_defaults_match_the_config_fixture() {
        let cfg = scratch_cfg();
        let st = SubtitleStyleState::load(&cfg);
        assert_eq!(st.color["primary"], "&H00FFFFFF");
        assert_eq!(st.color["border"], "H00000000");
        assert_eq!(st.margin["vertical"], 20);
        assert_eq!(st.alignment, 2);
    }

    #[test]
    fn subtitle_round_trips_and_reads_legacy_capitalized_alignment() {
        let mut cfg = scratch_cfg();
        // legacy capital-A spelling (what extras::ass wrote)
        cfg.set(
            "Additional",
            "subtitle_style",
            json!({"alignment": 8, "font": {"name": "宋体", "size": 30}}),
        );
        let st = SubtitleStyleState::load(&cfg);
        assert_eq!(st.alignment, 8);
        assert_eq!(st.font["name"], "宋体");

        let mut st = st;
        st.alignment = 7;
        st.save(&mut cfg).unwrap();
        let section = cfg.get("Additional", "subtitle_style").unwrap();
        assert_eq!(section["alignment"], 7);
        // the ASS reader's spelling stays in sync
        assert_eq!(section["Alignment"], 7);
        assert_eq!(
            SubtitleStyleState::load(&cfg).alignment,
            7,
            "lowercase wins on reload"
        );
    }

    #[test]
    fn alignment_map_covers_the_nine_keypad_slots() {
        assert_eq!(
            SUBTITLES_ALIGNMENT_MAP
                .iter()
                .map(|(_, v)| *v)
                .collect::<Vec<_>>(),
            (1..=9).collect::<Vec<_>>()
        );
        let c = catalog(ResolvedLanguage::ZhCn);
        assert_eq!(alignment_label(c, "BOTTOM_CENTER", 2), "底部居中 (2)");
        assert_eq!(alignment_label(c, "TOP_LEFT", 7), "顶部左对齐 (7)");
    }

    #[test]
    fn captions_translate() {
        let c = catalog(ResolvedLanguage::ZhCn);
        assert_eq!(danmaku_caption(c), "自定义弹幕样式");
        assert_eq!(subtitle_caption(c), "自定义字幕样式");
    }

    #[test]
    fn row_labels_translate() {
        let c = catalog(ResolvedLanguage::ZhCn);
        assert_eq!(c.tr("BorderGroup", "Outline (px)"), "边框 (像素)");
        assert_eq!(c.tr("FontGroup", "Font Size"), "字体大小");
    }
}
