//! `test_search_url.py` — search-keyword round-trips, ported verbatim.
//!
//! 2.14.0+ the keyword always travels in the link; paging and parse history
//! re-parse the same link. A bug here shows up as "page 2 lost the search"
//! or "parse history reopens the full list", not as "search broke".
use urls::search::{build_search_url, extract_keyword, keyword_param, support_search};

const SPACE: &str = "https://space.bilibili.com/123";
const FAVLIST: &str = "https://space.bilibili.com/123/favlist";
const WATCH_LATER: &str = "bilidown://watch_later";
const HISTORY: &str = "bilidown://history";
const VIDEO: &str = "https://www.bilibili.com/video/BV1xx411c7mD";
const COLLECTION: &str = "https://www.bilibili.com/list/456";

#[test]
fn supported() {
    for (url, param) in [
        (SPACE, "keyword"),
        (FAVLIST, "keyword"),
        (HISTORY, "keyword"),
        (WATCH_LATER, "key"), // 稍后再看接口用的参数名与其余三者不同
    ] {
        assert_eq!(keyword_param(url), Some(param), "keyword param for {url}");
        assert!(support_search(url), "support_search({url})");
    }
}

#[test]
fn unsupported() {
    // 合集、每周必看等接口没有搜索参数，必须明确返回不支持，
    // 否则界面会给出服务端搜索的入口，实际却只能筛当前页
    for url in [VIDEO, COLLECTION, "https://example.com/"] {
        assert_eq!(keyword_param(url), None, "keyword param for {url}");
        assert!(!support_search(url), "support_search({url})");
    }
}

#[test]
fn extract_percent_decoding() {
    assert_eq!(
        extract_keyword(&format!("{SPACE}?keyword=%E6%B5%8B%E8%AF%95")).as_deref(),
        Some("测试")
    );
}

#[test]
fn extract_uses_type_specific_param() {
    assert_eq!(
        extract_keyword(&format!("{WATCH_LATER}?key=abc")).as_deref(),
        Some("abc")
    );
    // 参数名不匹配时不应误读 wrong param name must not be misread
    assert_eq!(extract_keyword(&format!("{WATCH_LATER}?keyword=abc")), None);
}

#[test]
fn extract_strips_whitespace() {
    assert_eq!(
        extract_keyword(&format!("{SPACE}?keyword=%20abc%20")).as_deref(),
        Some("abc")
    );
}

#[test]
fn extract_absent_and_unsupported() {
    assert_eq!(extract_keyword(SPACE), None);
    assert_eq!(extract_keyword(&format!("{VIDEO}?keyword=x")), None);
}

#[test]
fn build_write() {
    assert_eq!(
        extract_keyword(&build_search_url(SPACE, "测试")).as_deref(),
        Some("测试")
    );
}

#[test]
fn build_overwrite() {
    assert_eq!(
        extract_keyword(&build_search_url(&format!("{SPACE}?keyword=old"), "new")).as_deref(),
        Some("new")
    );
}

#[test]
fn build_empty_keyword_removes_param() {
    // 清空关键词即恢复完整列表，参数必须被移除而不是留成 keyword=
    assert!(!build_search_url(&format!("{SPACE}?keyword=old"), "").contains("keyword"));
}

#[test]
fn build_unsupported_url_returned_unchanged() {
    assert_eq!(build_search_url(VIDEO, "x"), VIDEO);
}

#[test]
fn build_preserves_other_params() {
    let result = build_search_url(&format!("{SPACE}?tid=1&order=pubdate"), "测试");
    assert!(result.contains("tid=1"), "{result}");
    assert!(result.contains("order=pubdate"), "{result}");
    assert_eq!(extract_keyword(&result).as_deref(), Some("测试"));
}

#[test]
fn build_roundtrip() {
    // 关键词里可能出现任何字符，写入再读出必须完全一致，
    // 否则翻页会带着被破坏的关键词去请求接口
    for keyword in ["测试", "a b", "a&b=c", "100%", "a/b"] {
        assert_eq!(
            extract_keyword(&build_search_url(SPACE, keyword)).as_deref(),
            Some(keyword),
            "roundtrip {keyword}"
        );
    }
}
