//! External-behavior tests for the audio parser (spec Testing Decisions
//! #1): wiremock at the HTTP seam, fixtures mirroring real API shapes,
//! and the `TestAudioParser` vectors from
//! `test/test_episode_list_parsers.py` ported verbatim.

use parsers::tree::{Attribute, Number};
use parsers::{build_audio_tree, parse_audio, AudioError};
use serde_json::{json, Value};
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

/// A track row as `song/of-menu` returns it (fields the parser reads).
fn track(sid: i64, title: &str) -> Value {
    json!({
        "cover": format!("cover-{sid}.jpg"),
        "duration": 60,
        "passtime": 1700000000,
        "title": title,
        "author": "歌手",
        "statistic": {"sid": sid},
    })
}

#[tokio::test]
async fn au_single_fetches_song_info_and_builds_one_track() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/audio/music-service-c/web/song/info"))
        .and(query_param("sid", "111"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": track(111, "歌曲A"),
        })))
        .expect(1)
        .mount(&server)
        .await;

    let info = parse_audio(&client(), &server.uri(), "https://www.bilibili.com/audio/au111")
        .await
        .unwrap();

    // Single-song response carries no `data` list — the episode layer
    // normalizes the payload itself into the one track.
    assert!(info.data.get("data").is_none());
    assert_eq!(info.data["title"], "歌曲A");

    let root = build_audio_tree(&info.data);
    assert_eq!(root.count(), 1);
    let item = root.child(0);
    assert_eq!(item.title, "歌曲A");
    assert_eq!(item.sid, 111);
    assert_eq!(item.url, "https://www.bilibili.com/audio/au111");
    assert_eq!(item.duration, 60);
    assert_eq!(item.pubtime, 1700000000);
    assert_eq!(item.uploader, "歌手");
    assert_eq!(item.cover, "cover-111.jpg");
    assert_eq!(item.number, Number::Int(1));
    assert!(item.has_attribute(Attribute::AUDIO_BIT));
    // Menu title absent on singles → empty source_title.
    assert_eq!(item.related_titles["source_title"], "");

    server.verify().await;
}

#[tokio::test]
async fn am_menu_fetches_list_then_info_and_expands_tracks() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/audio/music-service-c/web/song/of-menu"))
        .and(query_param("sid", "55"))
        .and(query_param("pn", "1"))
        .and(query_param("ps", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {"data": [track(111, "歌曲A"), track(222, "歌曲B")]},
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/audio/music-service-c/web/menu/info"))
        .and(query_param("sid", "55"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {"title": "我的歌单"},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let info = parse_audio(&client(), &server.uri(), "https://www.bilibili.com/audio/am55")
        .await
        .unwrap();

    // Menu title is folded in as a sibling key of the track list.
    assert_eq!(info.data["menu_title"], "我的歌单");
    assert_eq!(info.data["data"].as_array().unwrap().len(), 2);

    let root = build_audio_tree(&info.data);
    assert!(root.has_attribute(Attribute::TREE_NODE_BIT));
    assert_eq!(root.title, "我的歌单");
    assert_eq!(root.count(), 2);

    for (index, sid) in [111, 222].iter().enumerate() {
        let item = root.child(index);
        assert_eq!(item.sid, *sid);
        assert_eq!(item.number, Number::Int(index as i64 + 1));
        assert_eq!(item.url, format!("https://www.bilibili.com/audio/au{sid}"));
        assert!(item.has_attribute(Attribute::AUDIO_BIT));
        assert!(!item.has_attribute(Attribute::TREE_NODE_BIT));
        assert_eq!(item.related_titles["source_title"], "我的歌单");
    }

    server.verify().await;
}

/// `TestAudioParser.test_song_title_written_to_source_title` — the audio
/// vector from test_episode_list_parsers.py, verbatim DATA payload.
#[test]
fn song_title_written_to_source_title() {
    let data = json!({
        "menu_title": "我的歌单",
        "data": [
            {"cover": "c1", "duration": 60, "passtime": 1700000000, "title": "歌曲A",
             "author": "歌手", "statistic": {"sid": 111}},
        ],
    });

    let root = build_audio_tree(&data);
    let item = root.child(0);

    assert_eq!(item.related_titles["source_title"], "我的歌单");
    assert_eq!(item.title, "歌曲A");
    assert_eq!(item.cover, "c1");
    assert_eq!(item.duration, 60);
    assert_eq!(item.pubtime, 1700000000);
    assert_eq!(item.sid, 111);
    assert_eq!(item.uploader, "歌手");
    assert_eq!(item.url, "https://www.bilibili.com/audio/au111");
    assert_eq!(item.number, Number::Int(1));
    assert!(item.has_attribute(Attribute::AUDIO_BIT));
}

/// Audio numbering rule shared by the flat-list parsers
/// (`TestNumbering`): starts at 1, increments in list order.
#[test]
fn numbering_starts_at_one_and_increments_in_order() {
    let data = json!({
        "menu_title": "m",
        "data": [
            {"cover": "c", "duration": 1, "passtime": 1, "title": "t1",
             "author": "a", "statistic": {"sid": 1}},
            {"cover": "c", "duration": 1, "passtime": 1, "title": "t2",
             "author": "a", "statistic": {"sid": 2}},
            {"cover": "c", "duration": 1, "passtime": 1, "title": "t3",
             "author": "a", "statistic": {"sid": 3}},
        ],
    });

    let root = build_audio_tree(&data);
    let numbers: Vec<Number> = (0..3).map(|i| root.child(i).number.clone()).collect();
    assert_eq!(
        numbers,
        vec![Number::Int(1), Number::Int(2), Number::Int(3)]
    );
}

#[tokio::test]
async fn non_zero_business_code_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/audio/music-service-c/web/song/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": -352, "message": "风控校验失败",
            "data": {},
        })))
        .mount(&server)
        .await;

    let error = parse_audio(&client(), &server.uri(), "au111")
        .await
        .unwrap_err();

    match error {
        AudioError::Api(message) => assert_eq!(message, "风控校验失败"),
        other => panic!("expected Api, got {other:?}"),
    }
}

#[tokio::test]
async fn non_audio_link_is_invalid() {
    let error = parse_audio(&client(), "https://example.com", "https://www.bilibili.com/video/BV1xx")
        .await
        .unwrap_err();
    assert!(matches!(error, AudioError::InvalidLink));
}
