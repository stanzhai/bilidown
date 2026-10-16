//! Download-time extras: subtitle export, NFO/JSON metadata sidecars and
//! cover export (ticket 18). Ports of `src/util/parse/additional/
//! {subtitles, metadata, cover, player}.py`, `file/{subtitle_ass,
//! metadata_nfo}.py`, the `_write` helper from `additional/base.py` and the
//! extras keys of `resolve()` in `src/util/download/task/options.py`.

pub mod ass;
pub mod cover;
pub mod nfo;
pub mod options;
pub mod player;
pub mod subtitles;
pub mod time;
pub mod writer;

/// i18n strings the extras pipeline must not hardcode: the qualifier words
/// baked into sidecar file names and the AI-generated subtitle track title
/// template (contains `{name}`). The app layer (ticket 19) supplies the
/// localized values; Python reads them from `Translator`.
#[derive(Debug, Clone)]
pub struct Labels {
    pub subtitles_qualifier: String,
    pub metadata_qualifier: String,
    pub ai_track_title: String,
}

/// Python truthiness: `False/None/0/0.0/""/[]/{}` are falsy.
pub(crate) fn python_truthy(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `f"{value}"` for floats: integral values keep a `.0` fraction (`1.0`,
/// `0.0`), the rest use the shortest round-trip form (`2.5`, `9.6`).
pub(crate) fn py_float(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}
