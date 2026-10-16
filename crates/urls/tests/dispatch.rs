//! `test_url_pattern.py` — dispatch table vectors, ported verbatim.
use urls::classify::{parser_type, ParserType};

fn pt(url: &str) -> Option<&'static str> {
    parser_type(url).map(ParserType::key)
}

#[test]
fn dispatch() {
    let cases = [
        // 完整链接 full links
        ("https://www.bilibili.com/video/BV1xx411c7mD", "video"),
        ("https://www.bilibili.com/bangumi/play/ep123", "bangumi"),
        ("https://www.bilibili.com/bangumi/media/md123", "bangumi"),
        ("https://www.bilibili.com/cheese/play/ss123", "cheese"),
        ("https://mall.bilibili.com/lesson/play?id=1", "lesson"),
        ("https://www.bilibili.com/v/popular/weekly", "popular"),
        ("https://www.bilibili.com/festival/abc", "festival"),
        ("https://b23.tv/abcdef", "b23"),
        ("https://bili2233.cn/abcdef", "b23"),
        // 顺序敏感 order-sensitive: all three also match the catch-all
        // space rule; only their earlier position distinguishes them.
        ("https://space.bilibili.com/123/lists", "list"),
        ("https://space.bilibili.com/123/favlist", "favlist"),
        ("https://space.bilibili.com/123", "space"),
        // 同样顺序敏感: /list/ml{id} is a favorites folder,
        // /list/{id} is a collection.
        ("https://www.bilibili.com/list/ml456", "favlist"),
        ("https://www.bilibili.com/list/456", "list"),
        // Collection/series play-page links: oid/bvid in the query must
        // not steal the type (a BV in the link matched by a later rule
        // would be parsed as a single video).
        ("https://www.bilibili.com/list/456?oid=789&bvid=BV1xx411c7mD", "list"),
        ("https://www.bilibili.com/list/456?sid=789", "list"),
        ("https://www.bilibili.com/list/456?sid=789&oid=123&bvid=BV1xx411c7mD", "list"),
        // 程序内部使用的伪协议 internal pseudo-protocols
        ("bili23://watch_later", "watch_later"),
        ("bili23://history", "history"),
        // 裸 ID bare ids
        ("BV1xx411c7mD", "video"),
        ("av123", "video"),
        ("ep456", "bangumi"),
        ("ss456", "bangumi"),
        ("au789", "audio"),
        ("am789", "audio"),
    ];
    for (url, expected) in cases {
        assert_eq!(pt(url), Some(expected), "dispatch failed for {url}");
    }
}

#[test]
fn query_string_does_not_affect_dispatch() {
    // 搜索关键词随链接传递，附加查询参数不得改变解析类型
    assert_eq!(pt("https://space.bilibili.com/123?keyword=x"), Some("space"));
    assert_eq!(pt("https://space.bilibili.com/123/favlist?keyword=x"), Some("favlist"));
}

#[test]
fn unknown_url() {
    assert_eq!(pt("https://example.com/"), None);
}

#[test]
fn raw_ids() {
    use urls::classify::{AudioId, BangumiId, CheeseId, RawIds};

    let ids = |url: &str| classify_ids(url);
    // Video
    assert_eq!(
        ids("https://www.bilibili.com/video/BV1xx411c7mD"),
        RawIds::Video { av: None, bv: Some("BV1xx411c7mD".into()) }
    );
    assert_eq!(ids("av123"), RawIds::Video { av: Some(123), bv: None });
    // Bangumi
    assert_eq!(
        ids("https://www.bilibili.com/bangumi/play/ep123"),
        RawIds::Bangumi { kind: BangumiId::Ep, id: 123 }
    );
    assert_eq!(ids("https://www.bilibili.com/bangumi/media/md123"), RawIds::Bangumi { kind: BangumiId::Md, id: 123 });
    assert_eq!(ids("ss456"), RawIds::Bangumi { kind: BangumiId::Ss, id: 456 });
    // Cheese
    assert_eq!(ids("https://www.bilibili.com/cheese/play/ss123"), RawIds::Cheese { kind: CheeseId::Ss, id: 123 });
    assert_eq!(ids("https://www.bilibili.com/cheese/play/ep123"), RawIds::Cheese { kind: CheeseId::Ep, id: 123 });
    // Lesson: ids come from courseId/lessonId/itemId query params; links
    // without them carry 0 (the parser layer rejects those as invalid).

    assert_eq!(
        ids("https://mall.bilibili.com/lesson/play?id=1"),
        RawIds::Lesson { course_id: 0, lesson_id: 0, item_id: 0 }
    );
    assert_eq!(
        ids("https://mall.bilibili.com/lesson/play?courseId=1&lessonId=2&itemId=3"),
        RawIds::Lesson { course_id: 1, lesson_id: 2, item_id: 3 }
    );
    // List
    assert_eq!(
        ids("https://www.bilibili.com/list/456?sid=789&oid=123&bvid=BV1xx411c7mD"),
        RawIds::List {
            id: 456,
            season: None,
            series: None,
            sid: Some(789),
            oid: Some(123),
            bvid: Some("BV1xx411c7mD".into()),
        }
    );
    // Favlist: /list/ml{id} carries the media id
    assert_eq!(ids("https://www.bilibili.com/list/ml456"), RawIds::Favlist { media_id: Some(456), owner_mid: None });
    // Space
    assert_eq!(ids("https://space.bilibili.com/123"), RawIds::Space { mid: 123 });
    // Popular
    assert_eq!(ids("https://www.bilibili.com/v/popular/weekly?num=42"), RawIds::Popular { number: Some(42) });
    assert_eq!(ids("https://www.bilibili.com/v/popular/weekly"), RawIds::Popular { number: None });
    // Audio
    assert_eq!(ids("au789"), RawIds::Audio { kind: AudioId::Au, id: 789 });
    assert_eq!(ids("am789"), RawIds::Audio { kind: AudioId::Am, id: 789 });
    // Short links resolve to RawIds::ShortLink — the ids live behind
    // the redirect, so nothing sensible can be extracted yet.
    assert_eq!(ids("https://b23.tv/abcdef"), RawIds::ShortLink);
    assert_eq!(ids("https://www.bilibili.com/festival/abc"), RawIds::ShortLink);
}

fn classify_ids(url: &str) -> urls::classify::RawIds {
    urls::classify::classify(url).expect("url must match a table row").ids
}
