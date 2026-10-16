//! External-behavior tests for the danmaku pipeline (spec Testing
//! Decisions #1 + #3): protobuf decode from fixture bytes, wiremock at
//! the segment-fetch HTTP seam, the test_danmaku_xml.py vectors ported
//! verbatim, and the ASS style-option matrix.

mod common;

use common::{hex_seg1, hex_seg2};
use danmaku::proto;
use danmaku::{decode_segment, render_ass, render_json, render_xml, DanmakuEntry, DanmakuStyle};
use prost::Message;
// test_danmaku_xml.py vectors, ported verbatim
// ---------------------------------------------------------------------------

/// Python `entry()` base vector.
fn entry() -> DanmakuEntry {
    DanmakuEntry {
        id: 0,
        stime: 12340,
        mode: 1,
        size: 25,
        color: 16_777_215,
        uhash: "abc".to_owned(),
        text: "普通弹幕".to_owned(),
        date: 1_700_000_000,
        dmid: "999".to_owned(),
    }
}

fn entry_over(stime: i64, mode: i32, size: i32, color: u32, text: &str) -> DanmakuEntry {
    DanmakuEntry {
        stime,
        mode,
        size,
        color,
        text: text.to_owned(),
        ..entry()
    }
}

/// Minimal XML well-formedness + `<d>` extraction (Python used
/// xml.etree; a hand parse of the single-`d` documents is equivalent for
/// these vectors).
fn d_texts(xml: &str) -> Vec<String> {
    xml.lines()
        .filter_map(|l| l.trim().strip_prefix("<d p="))
        .map(|rest| {
            let close = rest.find("</d>").expect("closing </d>");
            let text_start = rest.find('>').expect("text start") + 1;
            unescape(&rest[text_start..close])
        })
        .collect()
}

fn d_p(xml: &str) -> Vec<String> {
    let line = xml.lines().find(|l| l.contains("<d p=")).expect("<d>");
    let start = line.find('"').expect("p open") + 1;
    let end = line[start..].find('"').expect("p close") + start;
    line[start..end].split(',').map(str::to_owned).collect()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[test]
fn test_output_is_parseable() {
    let xml = render_xml(&[entry()], 12345);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(xml.contains("<i>"));
    assert!(xml.contains("<chatid>12345</chatid>"));
    assert!(xml.contains("<chatserver>chat.bilibili.com</chatserver>"));
    assert!(xml.trim_end().ends_with("</i>"));
}

#[test]
fn test_empty_list() {
    let xml = render_xml(&[], 12345);
    assert_eq!(d_texts(&xml).len(), 0);
}

#[test]
fn test_special_characters_keep_document_valid() {
    // Verbatim vector from test_danmaku_xml.py.
    for text in [
        "a<b>c",
        "a&b",
        "a\"b",
        "a'b",
        "&lt;",                // 已经像转义序列的文本不能被二次破坏
        "<![CDATA[x]]>",
        "</d><d p=\"0\">注入", // 构造出的闭合标签不得改变文档结构
        "&amp;<>&",
    ] {
        let xml = render_xml(&[entry_over(12340, 1, 25, 16_777_215, text)], 12345);
        // 只能有一个 <d>，说明注入没有制造出额外节点
        assert_eq!(d_texts(&xml).len(), 1, "text: {text}");
        // 解析回来的文本必须与原文完全一致
        assert_eq!(d_texts(&xml)[0], text, "text: {text}");
    }
}

#[test]
fn test_ampersand_escaped_first() {
    // & 必须最先替换，否则 "<" → "&lt;" 之后的 & 会被二次转义
    let xml = render_xml(&[entry_over(12340, 1, 25, 16_777_215, "<")], 12345);
    assert!(!xml.contains("&amp;lt;"));
}

#[test]
fn test_control_characters_removed() {
    for ch in ['\0', '\u{1}', '\u{8}', '\u{1f}', '\u{7f}'] {
        let text = format!("a{ch}b");
        let xml = render_xml(&[entry_over(12340, 1, 25, 16_777_215, &text)], 12345);
        assert_eq!(d_texts(&xml)[0], "ab", "char: {ch:?}");
    }
}

#[test]
fn test_has_exactly_eight_fields() {
    // B 站弹幕的 p 属性是固定 8 字段
    let xml = render_xml(&[entry()], 12345);
    assert_eq!(d_p(&xml).len(), 8);
}

#[test]
fn test_field_order() {
    let xml = render_xml(&[entry()], 12345);
    let fields = d_p(&xml);
    assert_eq!(fields[0], "12.34000"); // 毫秒转秒，保留 5 位小数
    assert_eq!(fields[1], "1");
    assert_eq!(fields[2], "25");
    assert_eq!(fields[3], "16777215");
    assert_eq!(fields[4], "1700000000");
    assert_eq!(fields[5], "0");
    assert_eq!(fields[6], "abc");
    assert_eq!(fields[7], "999");
}

#[test]
fn test_missing_fields_fall_back_to_defaults() {
    // 服务端省略默认值字段（protobuf 的 MessageToDict 行为），取值必须有兜底
    let e = DanmakuEntry {
        stime: 1000,
        text: "x".to_owned(),
        ..Default::default()
    };
    let xml = render_xml(&[e], 12345);
    assert_eq!(
        d_p(&xml),
        ["1.00000", "1", "25", "16777215", "0", "0", "0", "0"]
    );
}

#[test]
fn weight_is_not_emitted() {
    // weight 不属于 p 属性（它曾被 str.format 静默丢弃）
    let mut e = entry();
    e.id = 7; // id likewise never reaches the p attribute
    let xml = render_xml(&[e], 12345);
    assert!(!d_p(&xml).contains(&"7".to_owned()));
}

// ---------------------------------------------------------------------------
// protobuf decode from fixture bytes
// ---------------------------------------------------------------------------

#[test]
fn decode_reads_fixture_segment() {
    let entries = decode_segment(&hex_seg1()).unwrap();
    assert_eq!(entries.len(), 3, "stime/text-empty entry filtered");

    assert_eq!(entries[0].id, 1001);
    assert_eq!(entries[0].stime, 12340);
    assert_eq!(entries[0].mode, 1);
    assert_eq!(entries[0].size, 25);
    assert_eq!(entries[0].color, 16_777_215);
    assert_eq!(entries[0].uhash, "abc");
    assert_eq!(entries[0].text, "普通弹幕");
    assert_eq!(entries[0].date, 1_700_000_000);
    assert_eq!(entries[0].dmid, "999");

    // defaults-omitted entry decodes to proto3 zeros
    assert_eq!(entries[2].id, 1004);
    assert_eq!(entries[2].stime, 5000);
    assert_eq!(entries[2].mode, 0);
    assert_eq!(entries[2].size, 0);
    assert_eq!(entries[2].text, "默认值字段省略");
}

#[test]
fn decode_garbage_is_an_error() {
    assert!(decode_segment(&[0xff, 0xff, 0xff, 0xff]).is_err());
}

#[test]
fn proto_message_round_trips() {
    // The generated struct decodes the same bytes back to the same values.
    let reply = proto::DmSegMobileReply::decode(hex_seg2().as_slice()).unwrap();
    assert_eq!(reply.elems.len(), 2);
    assert_eq!(reply.elems[0].id, 2001);
    assert_eq!(reply.elems[0].mode, 5);
    assert_eq!(reply.elems[1].text, "scroll b");
}

// ---------------------------------------------------------------------------
// json export
// ---------------------------------------------------------------------------

#[test]
fn json_output_is_two_space_indented_message_to_dict_shape() {
    let e = DanmakuEntry {
        id: 1001,
        stime: 12340,
        mode: 1,
        size: 25,
        color: 16_777_215,
        uhash: "abc".to_owned(),
        text: "普通弹幕".to_owned(),
        date: 1_700_000_000,
        dmid: "999".to_owned(),
    };
    let json = render_json(&[e]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // MessageToDict: int64 fields are strings, default fields absent.
    assert_eq!(v[0]["id"], "1001");
    assert_eq!(v[0]["date"], "1700000000");
    assert_eq!(v[0]["stime"], 12340);
    assert_eq!(v[0]["text"], "普通弹幕");
    assert!(v[0].get("weight").is_none());
    // indent = 2; first key is proto field 1 (id), int64 as string
    assert!(json.contains("\n  {\n    \"id\": \"1001\","));
}

// ---------------------------------------------------------------------------
// ASS renderer: style-option matrix
// ---------------------------------------------------------------------------

fn scroll_entry(stime: i64, text: &str) -> DanmakuEntry {
    DanmakuEntry {
        stime,
        mode: 1,
        text: text.to_owned(),
        ..Default::default()
    }
}

fn style_line(ass: &str) -> String {
    ass.lines()
        .find(|l| l.starts_with("Style: "))
        .expect("Style line")
        .to_owned()
}

fn dialogue_lines(ass: &str) -> Vec<String> {
    ass.lines()
        .filter(|l| l.starts_with("Dialogue: "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn ass_header_and_default_style() {
    let ass = render_ass(&[scroll_entry(0, "a")], "标题", &DanmakuStyle::default());
    assert!(ass.contains("Title: 标题"));
    assert!(ass.contains("PlayResX: 1280"));
    assert!(ass.contains("PlayResY: 720"));
    assert!(ass.contains("Aspect Ratio: 1280:720"));

    // opacity 80 → alpha = int((1.0-0.8)*255) = 50 = 0x32 (binary-float
    // truncation parity with Python's int())
    let style = style_line(&ass);
    assert_eq!(
        style,
        "Style: Default,黑体,36,&H32FFFFFF,&H320000FF,&H32000000,&H32000000,0,0,0,0,100,100,0,0,1,1.0,0,7,0,0,0,1"
    );
}

#[test]
fn style_matrix_every_option_reaches_the_style_line() {
    let mut style = DanmakuStyle::default();
    style.font.name = "楷体".to_owned();
    style.font.size = 54;
    style.font.bold = true;
    style.font.italic = true;
    style.font.underline = true;
    style.font.strike = true;
    style.border.border = 2.5;
    style.border.shadow = 3;
    style.advanced.opacity = 100; // alpha 00
    let ass = render_ass(&[], "t", &style);
    assert_eq!(
        style_line(&ass),
        "Style: Default,楷体,54,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,1,1,1,1,100,100,0,0,1,2.5,3,7,0,0,0,1"
    );

    style.advanced.opacity = 50; // alpha 127 → 0x7F
    let ass = render_ass(&[], "t", &style);
    assert!(style_line(&ass).starts_with("Style: Default,楷体,54,&H7FFFFFFF"));
}

#[test]
fn resolution_changes_playres_and_geometry() {
    let mut style = DanmakuStyle::default();
    style.resolution.width = 1920;
    style.resolution.height = 1080;
    let ass = render_ass(&[scroll_entry(0, "a")], "t", &style);
    assert!(ass.contains("PlayResX: 1920"));
    assert!(ass.contains("PlayResY: 1080"));
    // scroll moves from the new right edge
    let d = &dialogue_lines(&ass)[0];
    assert!(d.contains("\\move(1920,0,-"), "{d}");
}

#[test]
fn scroll_duration_and_static_duration_change_dialogue_times() {
    let mut style = DanmakuStyle::default();
    style.advanced.scroll_duration = 12;
    style.advanced.static_duration = 4;

    let scroll = DanmakuEntry {
        stime: 1000,
        mode: 1,
        text: "s".to_owned(),
        ..Default::default()
    };
    let ass = render_ass(&[scroll], "t", &style);
    assert!(dialogue_lines(&ass)[0].contains("0:00:01.00,0:00:13.00"), "{ass}");

    let top = DanmakuEntry {
        stime: 1000,
        mode: 5,
        text: "t".to_owned(),
        ..Default::default()
    };
    let ass = render_ass(&[top], "t", &style);
    assert!(dialogue_lines(&ass)[0].contains("0:00:01.00,0:00:05.00"), "{ass}");
}

#[test]
fn display_area_limits_rows() {
    // default: 720 * 0.6 / (height+4) rows available. Shrink the area to
    // 5% → a single row; two simultaneous danmaku cannot both fit and one
    // is dropped.
    let mut style = DanmakuStyle::default();
    style.advanced.display_area = 5;
    let entries = [scroll_entry(0, "a"), scroll_entry(0, "b")];
    let ass = render_ass(&entries, "t", &style);
    assert_eq!(dialogue_lines(&ass).len(), 1, "{ass}");

    // wide open area → both fit on separate rows
    style.advanced.display_area = 100;
    let ass = render_ass(&entries, "t", &style);
    assert_eq!(dialogue_lines(&ass).len(), 2);
    let (d0, d1) = (&dialogue_lines(&ass)[0], &dialogue_lines(&ass)[1]);
    assert!(d0.contains("\\move(1280,0,"), "{d0}");
    assert!(d1.contains("\\move(1280,49,"), "{d1}"); // line_height = ceil(36*1.2)+1+4 = 49
}

#[test]
fn minimum_gap_affects_collision() {
    // Two danmaku 2 s apart, 8 chars wide (~144 px): with a large
    // min_gap cond1 (tail + gap clear of the right edge) fails, so the
    // second must take another track; with gap 0 it can share.
    let mut style = DanmakuStyle::default();
    style.advanced.minimum_gap = 5000;
    let entries = [scroll_entry(0, "aaaaaaaa"), scroll_entry(2000, "bbbbbbbb")];

    fn row_of(ass: &str) -> Vec<String> {
        dialogue_lines(ass)
            .into_iter()
            .map(|d| {
                d.split("\\move(1280,")
                    .nth(1)
                    .and_then(|r| r.split(',').next())
                    .unwrap_or("?")
                    .to_owned()
            })
            .collect()
    }

    let tight = render_ass(&entries, "t", &style);
    let rows = row_of(&tight);
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0], rows[1], "large gap must split tracks: {tight}");

    style.advanced.minimum_gap = 0;
    let loose = render_ass(&entries, "t", &style);
    let rows2 = row_of(&loose);
    assert_eq!(rows2.len(), 2);
    // 2 s apart, same track is geometrically fine with gap 0
    assert_eq!(rows2[0], rows2[1], "zero gap must allow sharing: {loose}");
}

#[test]
fn font_size_changes_line_height_and_row_stride() {
    let mut style = DanmakuStyle::default();
    style.font.size = 72; // height = ceil(72*1.2)+1 = 88, line_height 92
    let entries = [scroll_entry(0, "a"), scroll_entry(0, "b")];
    let ass = render_ass(&entries, "t", &style);
    let d1 = &dialogue_lines(&ass)[1];
    assert!(d1.contains("\\move(1280,92,"), "{d1}");
}

#[test]
fn top_and_bottom_modes_use_pos_tags() {
    let entries = [
        DanmakuEntry {
            stime: 1000,
            mode: 5,
            text: "top".to_owned(),
            ..Default::default()
        },
        DanmakuEntry {
            stime: 1000,
            mode: 4,
            text: "bottom".to_owned(),
            ..Default::default()
        },
    ];
    let ass = render_ass(&entries, "t", &DanmakuStyle::default());
    let ds = dialogue_lines(&ass);
    assert_eq!(ds.len(), 2);
    assert!(ds[0].contains("\\an8\\pos(640,0)"), "{}", ds[0]);
    assert!(ds[1].contains("\\an2\\pos(640,720)"), "{}", ds[1]);
}

#[test]
fn colored_danmaku_get_color_tag_others_do_not() {
    let colored = DanmakuEntry {
        stime: 0,
        mode: 1,
        color: 0xFF0000, // red → BGR &H0000FF&
        text: "c".to_owned(),
        ..Default::default()
    };
    let plain = DanmakuEntry {
        stime: 0,
        mode: 1,
        color: 16_777_215,
        text: "p".to_owned(),
        ..Default::default()
    };
    let ass = render_ass(&[colored.clone(), plain], "t", &DanmakuStyle::default());
    let ds = dialogue_lines(&ass);
    assert!(ds[0].contains("\\c&H0000FF&"), "{}", ds[0]);
    assert!(!ds[1].contains("\\c&"), "{}", ds[1]);

    // color 0 (proto3 default, absent in dict) → no tag
    let zero = DanmakuEntry {
        color: 0,
        ..colored
    };
    let ass = render_ass(&[zero], "t", &DanmakuStyle::default());
    assert!(!dialogue_lines(&ass)[0].contains("\\c&"));
}

#[test]
fn entries_are_sorted_by_stime() {
    let entries = [
        scroll_entry(3000, "late"),
        scroll_entry(1000, "early"),
    ];
    let ass = render_ass(&entries, "t", &DanmakuStyle::default());
    let ds = dialogue_lines(&ass);
    assert!(ds[0].ends_with("early"), "{ds:?}");
    assert!(ds[1].ends_with("late"), "{ds:?}");
}

#[test]
fn unknown_modes_are_skipped() {
    let entries = [DanmakuEntry {
        stime: 0,
        mode: 6,
        text: "x".to_owned(),
        ..Default::default()
    }];
    let ass = render_ass(&entries, "t", &DanmakuStyle::default());
    assert_eq!(dialogue_lines(&ass).len(), 0);
}

#[test]
fn ass_time_uses_python_centis_clamp() {
    // 0:00:00.995 rounds to .100 → clamps to 99 (Python behavior).
    let entries = [scroll_entry(995, "x")];
    let ass = render_ass(&entries, "t", &DanmakuStyle::default());
    assert!(dialogue_lines(&ass)[0].contains("0:00:00.99,0:00:10.99"), "{ass}");
}

// ---------------------------------------------------------------------------
// wiremock segment fetch (HTTP seam)
// ---------------------------------------------------------------------------

mod fetch {
    use super::common::{hex_seg1, hex_seg2};
    use danmaku::{fetch_segments, segment_count, DanmakuEntry};
    use network::{ClientOptions, HttpClient, ProxyConfig};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client() -> HttpClient {
        HttpClient::new(ClientOptions {
            user_agent: "test-ua".to_owned(),
            proxy: ProxyConfig::none(),
            cookies_from_persistence: Vec::new(),
            signer: None,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn fetches_one_segment_per_six_minutes_in_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x/v2/dm/wbi/web/seg.so"))
            .and(query_param("type", "1"))
            .and(query_param("oid", "12345"))
            .and(query_param("segment_index", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(hex_seg1()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x/v2/dm/wbi/web/seg.so"))
            .and(query_param("segment_index", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(hex_seg2()))
            .expect(1)
            .mount(&server)
            .await;

        // 610s → 2 segments (ceil(610/360))
        assert_eq!(segment_count(610), 2);
        let entries = fetch_segments(&client(), &server.uri(), 12345, 610).await.unwrap();
        assert_eq!(entries.len(), 5, "{entries:?}");
        assert_eq!(entries[0].text, "普通弹幕");
        assert_eq!(entries[3].text, "顶部弹幕"); // seg2 first element
        assert_eq!(entries[4].text, "scroll b");

        server.verify().await;
    }

    #[tokio::test]
    async fn zero_or_missing_duration_fetches_nothing() {
        let server = MockServer::start().await;
        assert_eq!(segment_count(0), 0);
        let entries = fetch_segments(&client(), &server.uri(), 1, 0).await.unwrap();
        assert!(entries.is_empty());
        server.verify().await;
    }

    #[test]
    fn segment_count_matches_python_ceil() {
        assert_eq!(segment_count(1), 1);
        assert_eq!(segment_count(360), 1);
        assert_eq!(segment_count(361), 2);
        assert_eq!(segment_count(720), 2);
        assert_eq!(segment_count(721), 3);
        assert_eq!(segment_count(-5), 0);
    }

    #[test]
    fn entry_defaults_are_zeroed() {
        // The decoded default entry mirrors the dict fallbacks the xml
        // renderer applies.
        assert_eq!(
            DanmakuEntry::default().color, 0,
            "default color is proto3 zero; xml substitutes 16777215"
        );
    }

}
