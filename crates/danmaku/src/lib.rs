//! Danmaku pipeline (ticket 17): protobuf segment fetch + xml/ass/json export.
//!
//! Ports `src/util/parse/additional/danmaku.py` (fetch/decode/dedup and
//! format dispatch), `file/danmaku_xml.py` and `file/danmaku_ass.py`
//! (including the layout engine and every Danmaku Style option).

pub mod proto {
    //! prost-generated segment messages (subset of Bilibili `dm.proto`
    //! with field names/numbers verbatim).
    include!(concat!(
        env!("OUT_DIR"),
        "/bilibili.community.service.dm.v1.rs"
    ));
}

mod ass;
mod style;
mod xml;

pub use ass::render_ass;
pub use style::DanmakuStyle;
pub use xml::render_xml;

/// Segment endpoint (Python `_get_protobuf_danmaku`).
pub const SEG_API: &str = "https://api.bilibili.com/x/v2/dm/wbi/web/seg.so";

/// One decoded danmaku entry, the field subset Python's `MessageToDict`
/// list carried downstream (defaults = proto3 zeros, per
/// `test_missing_fields_fall_back_to_defaults`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DanmakuEntry {
    pub id: i64,
    /// Appear time in ms.
    pub stime: i64,
    /// 1-3 scroll, 4 bottom, 5 top.
    pub mode: i32,
    pub size: i32,
    pub color: u32,
    pub uhash: String,
    pub text: String,
    pub date: i64,
    pub dmid: String,
}

impl DanmakuEntry {
    fn from_proto(e: &proto::DanmakuElem) -> Self {
        Self {
            id: e.id,
            stime: e.stime as i64,
            mode: e.mode,
            size: e.size,
            color: e.color,
            uhash: e.uhash.clone(),
            text: e.text.clone(),
            date: e.date,
            dmid: e.dmid.clone(),
        }
    }

    /// JSON entry, `MessageToDict` shape with default fields omitted
    /// (proto3 semantics — only non-default fields appear).
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;

        let mut v = serde_json::Map::new();
        let mut put = |k: &str, val: serde_json::Value| {
            if val != serde_json::Value::Null {
                v.insert(k.to_owned(), val);
            }
        };
        if self.id != 0 {
            put("id", json!(self.id.to_string()));
        }
        if self.stime != 0 {
            put("stime", json!(self.stime));
        }
        if self.mode != 0 {
            put("mode", json!(self.mode));
        }
        if self.size != 0 {
            put("size", json!(self.size));
        }
        if self.color != 0 {
            put("color", json!(self.color));
        }
        if !self.uhash.is_empty() {
            put("uhash", json!(self.uhash));
        }
        if !self.text.is_empty() {
            put("text", json!(self.text));
        }
        if self.date != 0 {
            put("date", json!(self.date.to_string()));
        }
        if !self.dmid.is_empty() {
            put("dmid", json!(self.dmid));
        }
        serde_json::Value::Object(v)
    }
}

/// Segments per duration: one per ceil(360 s) (Python
/// `_get_all_protobuf_parts`).
pub fn segment_count(duration_secs: i64) -> usize {
    if duration_secs > 0 {
        (duration_secs as f64 / 360.0).ceil() as usize
    } else {
        0
    }
}

/// Decode one protobuf segment body, keeping only entries with both
/// `stime` and `text` set (Python `_proto_to_dict_list`'s filter).
pub fn decode_segment(bytes: &[u8]) -> Result<Vec<DanmakuEntry>, DecodeError> {
    use prost::Message;

    let reply = proto::DmSegMobileReply::decode(bytes)?;
    Ok(reply
        .elems
        .iter()
        .map(DanmakuEntry::from_proto)
        .filter(|e| e.stime != 0 && !e.text.is_empty())
        .collect())
}

#[derive(Debug, thiserror::Error)]
#[error("protobuf decode failed: {0}")]
pub struct DecodeError(#[from] prost::DecodeError);

/// Fetch every segment for `cid` through `client` (host-swapped for tests),
/// returning the decoded, stime/text-filtered entries in segment order.
///
/// Python fetches synchronously per index; sequential async keeps the same
/// request order.
pub async fn fetch_segments(
    client: &network::HttpClient,
    base_url: &str,
    cid: i64,
    duration_secs: i64,
) -> Result<Vec<DanmakuEntry>, FetchError> {
    let url = crate::swap_host(SEG_API, base_url);
    let mut entries = Vec::new();
    for index in 1..=segment_count(duration_secs) {
        let params: Vec<(String, String)> = vec![
            ("type".into(), "1".into()),
            ("oid".into(), cid.to_string()),
            ("segment_index".into(), index.to_string()),
        ];
        let body = client.get_bytes(&url, &params).await?;
        entries.extend(decode_segment(&body)?);
    }
    Ok(entries)
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error(transparent)]
    Http(#[from] network::HttpError),
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

/// Swap the endpoint's host for the wiremock `base` in tests
/// (`parsers::swap_host` twin; `base` empty → untouched URL).
pub fn swap_host(api: &str, base: &str) -> String {
    if base.is_empty() {
        return api.to_owned();
    }
    let path = api
        .strip_prefix("https://")
        .and_then(|rest| rest.find('/').map(|i| &rest[i..]))
        .unwrap_or(api);
    format!("{base}{path}")
}

/// Render the entries in the requested format (Python `parse()`'s
/// match on `DanmakuType`; xml needs `cid`, ass needs `title`+style).
pub fn render(
    entries: &[DanmakuEntry],
    format: DanmakuFormat,
    cid: i64,
    title: &str,
    style: &DanmakuStyle,
) -> String {
    match format {
        DanmakuFormat::Xml => render_xml(entries, cid),
        DanmakuFormat::Ass => render_ass(entries, title, style),
        DanmakuFormat::Json => render_json(entries),
    }
}

/// `DanmakuType` values (config `Additional.danmaku_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanmakuFormat {
    Xml,
    Ass,
    Json,
}

/// `dumps(dict_list, indent = 2)` — two-space-indented array of the
/// `MessageToDict` entries, non-ASCII kept verbatim.
pub fn render_json(entries: &[DanmakuEntry]) -> String {
    let items: Vec<serde_json::Value> = entries.iter().map(DanmakuEntry::to_json).collect();
    serde_json::to_string_pretty(&serde_json::Value::Array(items))
        .expect("serde_json never fails on Value")
}
