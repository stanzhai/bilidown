//! `hash_id` algorithm, ported verbatim from
//! `src/util/download/task/hash_id.py`. The parsing side and the persistence
//! side must produce identical values, so both build against this one
//! implementation. (ticket 03)

use crate::info::TaskInfo;
use md5::{Digest, Md5};

/// Bumped only when the algorithm itself changes; triggers a rehash of
/// existing rows via `PRAGMA user_version`.
pub const HASH_ID_VERSION: i64 = 1;

/// `Attribute` bits from `src/util/parse/episode/tree.py` that participate in
/// hash_id selection.
const VIDEO_BIT: i64 = 1 << 0;
const BANGUMI_BIT: i64 = 1 << 1;
const CHEESE_BIT: i64 = 1 << 2;
const AUDIO_BIT: i64 = 1 << 16;
const LESSON_BIT: i64 = 1 << 18;

/// Computes the duplicate-detection hash for a task. The metadata dict shape
/// per attribute branch and the stable-JSON digest match the Python build.
pub fn calc_hash_id(task_info: &TaskInfo) -> String {
    let ep = &task_info.Episode;
    let attribute = ep.attribute;

    let mut metadata = serde_json::Map::new();

    if attribute & VIDEO_BIT != 0 {
        // 投稿视频
        metadata.insert("bvid".into(), ep.bvid.clone().into());
        metadata.insert("cid".into(), ep.cid.into());
        metadata.insert("aid".into(), ep.aid.into());
    } else if attribute & BANGUMI_BIT != 0 {
        // 剧集类
        metadata.insert("bvid".into(), ep.bvid.clone().into());
        metadata.insert("cid".into(), ep.cid.into());
        metadata.insert("aid".into(), ep.aid.into());
        metadata.insert("ep_id".into(), ep.ep_id.into());
    } else if attribute & LESSON_BIT != 0 {
        metadata.insert("course_id".into(), ep.course_id.into());
        metadata.insert("lesson_id".into(), ep.lesson_id.into());
        metadata.insert("item_id".into(), ep.item_id.into());
        metadata.insert("section_id".into(), ep.section_id.into());
    } else if attribute & CHEESE_BIT != 0 {
        // 课程类
        metadata.insert("aid".into(), ep.aid.into());
        metadata.insert("cid".into(), ep.cid.into());
        metadata.insert("ep_id".into(), ep.ep_id.into());
    } else if attribute & AUDIO_BIT != 0 {
        // 音乐类
        metadata.insert("sid".into(), ep.sid.into());
    } else {
        // Unknown/missing attribute: still a distinguishable hash, keyed by
        // task_id so such records never collide with each other.
        metadata.insert("aid".into(), ep.aid.into());
        metadata.insert("bvid".into(), ep.bvid.clone().into());
        metadata.insert("cid".into(), ep.cid.into());
        metadata.insert("ep_id".into(), ep.ep_id.into());
        metadata.insert("sid".into(), ep.sid.into());
        metadata.insert("task_id".into(), task_info.Basic.task_id.clone().into());
    }

    stable_json_digest(&metadata)
}

/// Python `dumps_stable`: stdlib json, sorted keys, compact separators,
/// `ensure_ascii=True` (i.e. `\uXXXX` escapes), then MD5 of the UTF-8 bytes.
fn stable_json_digest(metadata: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut keys: Vec<&String> = metadata.keys().collect();
    keys.sort();

    let mut out = String::from("{");
    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&python_ascii(key));
        out.push(':');
        let value = match &metadata[*key] {
            serde_json::Value::String(s) => python_ascii(s),
            serde_json::Value::Number(n) => n.to_string(),
            _ => unreachable!("hash metadata values are strings or integers"),
        };
        out.push_str(&value);
    }
    out.push('}');

    let digest = Md5::digest(out.as_bytes());
    format!("{:x}", digest)
}

/// Escapes like Python `json.dumps(..., ensure_ascii=True)`: `"`/`\`/control
/// characters via the standard short escapes, every other non-ASCII char as
/// `\uXXXX` (astral chars as surrogate pairs).
fn python_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let cp = c as u32;
                if (0x10000..).contains(&cp) {
                    let v = cp - 0x10000;
                    out.push_str(&format!("\\u{:04x}", 0xD800 + (v >> 10)));
                    out.push_str(&format!("\\u{:04x}", 0xDC00 + (v & 0x3FF)));
                } else {
                    out.push_str(&format!("\\u{:04x}", cp));
                }
            }
        }
    }
    out.push('"');
    out
}
