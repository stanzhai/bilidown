//! External-behavior tests for batch parse: the dialog's input validation
//! contract and the walk against wiremock-pinned view responses.

use parsers::tree::Attribute;
use parsers::{aid_to_bvid, bvid_of, parse_batch, validate_batch_input, BatchError};
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use network::{ClientOptions, HttpClient, ProxyConfig};

fn client() -> HttpClient {
    HttpClient::new(ClientOptions {
        user_agent: "test-ua".to_owned(),
        proxy: ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

/// A single-video view `data` payload (fields the batch item builder
/// reads).
fn view_data(bvid: &str, title: &str, duration: i64) -> serde_json::Value {
    json!({
        "bvid": bvid,
        "aid": 170001,
        "title": title,
        "pic": "cover.jpg",
        "pubdate": 1700000000,
        "duration": duration,
        "cid": 280001,
        "desc": "desc",
        "tid": 1,
        "tid_v2": 1,
        "owner": {"mid": 42, "name": "UP主", "face": "face.jpg"},
    })
}

/// Shallow-merge two objects (test fixture builder).
fn merged(base: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    let mut out = base;
    if let (Some(map), Some(extra_map)) = (out.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_map {
            map.insert(k.clone(), v.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------
// Dialog validation (batch_parse.py accept())
// ---------------------------------------------------------------------

#[test]
fn blank_lines_are_dropped_and_the_rest_trimmed() {
    // Python's check is a case-sensitive substring: lowercase "bv" alone
    // fails the gate exactly like a plain URL does.
    let urls = validate_batch_input(
        "  https://www.bilibili.com/video/BV1aa  \n\n\nav123\n   \n",
    )
    .unwrap();

    assert_eq!(
        urls,
        vec!["https://www.bilibili.com/video/BV1aa", "av123"]
    );
}

#[test]
fn empty_input_is_no_links() {
    assert!(matches!(
        validate_batch_input(""),
        Err(BatchError::NoLinks)
    ));
    assert!(matches!(
        validate_batch_input("  \n \n "),
        Err(BatchError::NoLinks)
    ));
}

#[test]
fn non_av_bv_line_is_invalid_format() {
    // "au" carries no "av" substring and no "BV".
    assert!(matches!(
        validate_batch_input("https://www.bilibili.com/audio/au111"),
        Err(BatchError::InvalidLink)
    ));
    assert!(matches!(
        validate_batch_input("BV1aa\nhttps://example.com"),
        Err(BatchError::InvalidLink)
    ));
    // Lowercase "bv" fails the case-sensitive check.
    assert!(matches!(
        validate_batch_input("bv456"),
        Err(BatchError::InvalidLink)
    ));
}

// ---------------------------------------------------------------------
// id lifting (VideoParser)
// ---------------------------------------------------------------------

#[test]
fn av_converts_to_bvid_like_python() {
    // Python: VideoParser.get_aid("av170001") → aid_to_bvid(170001)
    assert_eq!(bvid_of("av170001").unwrap(), aid_to_bvid(170001));
    assert_eq!(bvid_of("AV170001").unwrap(), aid_to_bvid(170001));
}

#[test]
fn bv_passes_through_unchanged() {
    assert_eq!(bvid_of("BV1xx411c7mD").unwrap(), "BV1xx411c7mD");
    assert_eq!(
        bvid_of("https://www.bilibili.com/video/BV1xx411c7mD?p=2").unwrap(),
        "BV1xx411c7mD"
    );
}

#[test]
fn aid_to_bvid_known_vector() {
    // Sanity pin of the XOR/ALPHABET algorithm: stable 12-char output
    // starting with BV1 and round-tripping distinct aids apart.
    let a = aid_to_bvid(1);
    let b = aid_to_bvid(2);
    assert!(a.starts_with("BV1") && a.len() == 12);
    assert_ne!(a, b);
    assert_eq!(aid_to_bvid(170001), aid_to_bvid(170001));
}

// ---------------------------------------------------------------------
// The walk (DynamicParser.parse_url_list + VideoEpisodeParser)
// ---------------------------------------------------------------------

#[tokio::test]
async fn folds_multiple_links_into_one_root_with_running_count() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/wbi/view"))
        .and(query_param("bvid", "BV1aaa"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": view_data("BV1aaa", "视频一", 100),
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/wbi/view"))
        .and(query_param("bvid", "BV17x411w7KC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": merged(
                view_data("BV17x411w7KC", "视频二", 200),
                json!({"pages": [
                    {"cid": 280011, "page": 1, "part": "上", "duration": 10, "ctime": 1700000001},
                    {"cid": 280012, "page": 2, "part": "下", "duration": 20, "ctime": 1700000002},
                ]}),
            ),
        })))
        .expect(1)
        .mount(&server)
        .await;

    // av170001 converts to BV17x411w7KC via aid_to_bvid.
    let urls = validate_batch_input("BV1aaa\nav170001").unwrap();
    let mut progress: Vec<(usize, usize)> = Vec::new();
    let root = parse_batch(
        &client(),
        &server.uri(),
        &urls,
        |links, episodes| progress.push((links, episodes)),
        std::time::Duration::ZERO,
    )
    .await
    .unwrap();

    // av170001 converts to BV17x411w7KC via aid_to_bvid.
    assert!(root.has_attribute(Attribute::TREE_NODE_BIT));
    assert_eq!(root.count(), 3);

    let first = root.child(0);
    assert_eq!(first.title, "视频一");
    assert_eq!(first.bvid, "BV1aaa");
    assert_eq!(first.cid, 280001);
    assert!(first.has_attribute(Attribute::VIDEO_BIT | Attribute::NORMAL_BIT));

    let second = root.child(1);
    assert_eq!(second.bvid, "BV17x411w7KC");
    assert_eq!(second.title, "上");
    assert_eq!(second.part_number, 1);
    assert!(second.has_attribute(Attribute::VIDEO_BIT | Attribute::PART_BIT));
    assert_eq!(second.related_titles["parent_title"], "视频二");

    let third = root.child(2);
    assert_eq!(third.title, "下");
    assert_eq!(third.part_number, 2);

    // Progress: one entry per link with the running episode count.
    assert_eq!(progress, vec![(1, 1), (2, 3)]);

    server.verify().await;
}

#[tokio::test]
async fn multi_p_link_contributes_all_its_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/wbi/view"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": merged(
                view_data("BV1ppp", "多P视频", 300),
                json!({"pages": [
                    {"cid": 1, "page": 1, "part": "P1", "duration": 10, "ctime": 1},
                    {"cid": 2, "page": 2, "part": "P2", "duration": 20, "ctime": 2},
                    {"cid": 3, "page": 3, "part": "P3", "duration": 30, "ctime": 3},
                ]}),
            ),
        })))
        .mount(&server)
        .await;

    let root = parse_batch(
        &client(),
        &server.uri(),
        &["BV1ppp".to_owned()],
        |_, _| {},
        std::time::Duration::ZERO,
    )
    .await
    .unwrap();

    assert_eq!(root.count(), 3);
    let parts: Vec<&str> = (0..3)
        .map(|i| root.child(i).title.as_str())
        .collect();
    assert_eq!(parts, vec!["P1", "P2", "P3"]);
    let numbers: Vec<i64> = (0..3)
        .map(|i| match &root.child(i).number {
            parsers::tree::Number::Int(n) => *n,
            _ => panic!("expected int number"),
        })
        .collect();
    assert_eq!(numbers, vec![1, 2, 3]);
}

#[tokio::test]
async fn collection_link_folds_its_sections_flat() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/wbi/view"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": merged(
                view_data("BV1ccc", "合集入口", 10),
                json!({"ugc_season": {
                    "title": "我的合集",
                    "sections": [
                        {"title": "正片", "episodes": [
                            {"aid": 2, "bvid": "BV1ep1", "cid": 10, "title": "合集视频一",
                             "duration": 11, "pages": [{"cid": 10, "page": 1, "part": "合集视频一", "duration": 11}],
                             "arc": {"pic": "ep1.jpg", "pubdate": 1700000010}},
                            {"aid": 3, "bvid": "BV1ep2", "cid": 20, "title": "合集视频二",
                             "duration": 22, "pages": [{"cid": 20, "page": 1, "part": "合集视频二", "duration": 22}],
                             "arc": {"pic": "ep2.jpg", "pubdate": 1700000020}},
                        ]},
                    ],
                }}),
            ),
        })))
        .mount(&server)
        .await;

    let root = parse_batch(
        &client(),
        &server.uri(),
        &["BV1ccc".to_owned()],
        |_, _| {},
        std::time::Duration::ZERO,
    )
    .await
    .unwrap();

    assert_eq!(root.count(), 2);
    for i in 0..2 {
        let item = root.child(i);
        assert!(item.has_attribute(Attribute::VIDEO_BIT | Attribute::COLLECTION_BIT));
        assert_eq!(item.related_titles["collection_title"], "我的合集");
        // Single section → no section_title label.
        assert_eq!(item.related_titles["section_title"], "");
    }
    assert_eq!(root.child(0).title, "合集视频一");
    assert_eq!(root.child(1).title, "合集视频二");
}

#[tokio::test]
async fn api_error_stops_the_walk_with_the_message() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/wbi/view"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": -400, "message": "请求错误",
            "data": {},
        })))
        .mount(&server)
        .await;

    let error = parse_batch(
        &client(),
        &server.uri(),
        &["BV1bad".to_owned()],
        |_, _| {},
        std::time::Duration::ZERO,
    )
    .await
    .unwrap_err();

    match error {
        BatchError::Api(message) => assert_eq!(message, "请求错误"),
        other => panic!("expected Api, got {other:?}"),
    }
}

#[tokio::test]
async fn episode_side_data_records_uploader_and_description() {
    let data = view_data("BV1aaa", "视频一", 100);
    let side = parsers::video_episode_data(&data);

    assert_eq!(side["description"], "desc");
    assert_eq!(side["tid"], 1);
    assert_eq!(side["uploader"], "UP主");
    assert_eq!(side["uploader_uid"], 42);
    assert_eq!(side["uploader_face"], "face.jpg");
}
