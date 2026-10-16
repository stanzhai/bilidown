//! XML export — port of `src/util/parse/additional/file/danmaku_xml.py`.
//! The test contract is structural (real XML parse), not byte equality.

use crate::DanmakuEntry;
use std::fmt::Write;

const XML_BASE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<i>
    <chatserver>chat.bilibili.com</chatserver>
    <chatid>{cid}</chatid>
    <mission>0</mission>
    <maxlimit>1500</maxlimit>
    <state>0</state>
    <real_name>0</real_name>
    <source>k-v</source>
{comments}
</i>";

/// Generate the Bilibili-format danmaku XML document.
pub fn render_xml(entries: &[DanmakuEntry], cid: i64) -> String {
    let comments = comments(entries);
    XML_BASE
        .replace("{cid}", &cid.to_string())
        .replace("{comments}", &comments)
}

/// `entry.get("uhash", 0)` / `entry.get("dmid", 0)`: absent fields render
/// the number zero (Python formats the int default through the same
/// format string).
fn str_or_zero(s: &str) -> &str {
    if s.is_empty() {
        "0"
    } else {
        s
    }
}

/// Python `entry.get(key, default)`: absent/zero → default. The defaults
/// match the p-attribute vector in test_danmaku_xml.py.
fn or_default(v: i32, default: i32) -> i32 {
    if v == 0 {
        default
    } else {
        v
    }
}

fn comments(entries: &[DanmakuEntry]) -> String {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        // <d p="stime,mode,size,color,date,pool,uhash,dmid">text</d>,
        // indented one level (4 spaces) like Python's `textwrap.indent`.
        let mut line = String::new();
        let _ = write!(
            line,
            "    <d p=\"{},{},{},{},{},0,{},{}\">{}</d>",
            ms_to_s(e.stime),
            or_default(e.mode, 1),
            or_default(e.size, 25),
            if e.color == 0 { 16_777_215 } else { e.color },
            e.date,
            str_or_zero(&e.uhash),
            str_or_zero(&e.dmid),
            escape(&e.text)
        );
        out.push(line);
    }
    out.join("\n")
}

/// Remove XML 1.0-illegal control chars, then escape `& < > " '`
/// (`&` first — test_ampersand_escaped_first).
fn escape(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|c| {
            let b = *c as u32;
            !(b < 0x20 || b == 0x7f)
        })
        .collect();
    let mut out = String::with_capacity(filtered.len());
    for c in filtered.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// `f"{ms / 1000:.5f}"` — ms are exact in binary (÷1000 shifts the
/// decimal only), so the 5-decimal digits are the ms with trailing
/// zeros, and the carry case cannot occur.
fn ms_to_s(ms: i64) -> String {
    let neg = ms < 0;
    let ms = ms.unsigned_abs();
    let secs = ms / 1000;
    let frac = ms % 1000;
    let int_part = if neg {
        format!("-{secs}")
    } else {
        secs.to_string()
    };
    format!("{int_part}.{frac:03}00")
}
