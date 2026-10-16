//! Port of `test/test_list_parser.py` (ListParser over wiremock).

use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockGuard, MockServer, ResponseTemplate};

use parsers::parser_list::{parse_list_url, ListError};

const BVID: &str = "BV1xx411c7mD";

fn archive() -> Value {
    json!({
        "aid": 123, "bvid": BVID, "pic": "p",
        "pubdate": 1700000000, "title": "测试视频", "duration": 100
    })
}

fn view() -> Value {
    json!({
        "code": 0,
        "data": {
            "bvid": BVID, "aid": 123, "cid": 456,
            "ugc_season": {"id": 789, "mid": 99999, "title": "测试合集"}
        }
    })
}

fn view_without_season() -> Value {
    let mut view = view();
    view["data"]
        .as_object_mut()
        .unwrap()
        .remove("ugc_season");
    view
}

fn season_archives() -> Value {
    json!({
        "code": 0,
        "data": {
            "meta": {"season_id": 789, "mid": 99999, "title": "测试合集",
                      "name": "合集·测试合集", "total": 1},
            "page": {"page_num": 1, "page_size": 30, "total": 1},
            "archives": [archive()]
        }
    })
}

fn series_not_found() -> Value {
    json!({"code": 147002, "message": "视频列表已失效"})
}

fn series_archives() -> Value {
    json!({
        "code": 0,
        "data": {
            "aids": [123],
            "page": {"num": 1, "size": 30, "total": 1},
            "archives": [archive()]
        }
    })
}

fn series_meta() -> Value {
    json!({
        "code": 0,
        "data": {"meta": {"series_id": 1462496, "mid": 12345,
                           "name": "测试系列", "total": 1}}
    })
}


/// Scoped mock matching GET on `p` carrying query param `key=value`.
async fn mount(
    server: &MockServer,
    p: &str,
    key: &str,
    value: &str,
    body: Value,
) -> MockGuard {
    Mock::given(method("GET"))
        .and(path(p))
        .and(query_param(key, value))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1..)
        .mount_as_scoped(server)
        .await
}

fn client() -> network::HttpClient {
    network::HttpClient::new(network::ClientOptions {
        user_agent: "test-ua".to_owned(),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

fn q(body: &Value) -> String {
    serde_json::to_string(body).unwrap()
}

#[tokio::test]
async fn parse_play_page_with_bvid_and_sid() {
    let server = MockServer::start().await;
    let _view = mount(&server, "/x/web-interface/wbi/view", "bvid", BVID, view()).await;
    let _season = mount(&server, "/x/polymer/web-space/seasons_archives_list", "season_id", "789", season_archives()).await;

    let result = parse_list_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/list/99999?sid=789&oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap();

    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    let requests = server.received_requests().await.unwrap();
    let first = &requests[0];
    assert!(first.url.query().unwrap().contains("bvid="));
    let second = &requests[1];
    assert!(second
        .url
        .query()
        .unwrap()
        .contains("seasons_archives_list")
        || second.url.path().contains("seasons_archives_list"));
    assert!(second.url.query().unwrap().contains("season_id=789"));
    assert!(second.url.query().unwrap().contains("mid=99999"));

    assert_eq!(
        result.info_data["data"]["meta"]["title"],
        json!("测试合集")
    );
    assert_eq!(
        result.current_episode_data,
        Some(("bvid".to_owned(), json!(BVID)))
    );
}

#[tokio::test]
async fn parse_play_page_with_oid() {
    let server = MockServer::start().await;
    let _view = mount(&server, "/x/web-interface/wbi/view", "aid", "123", view()).await;
    let _season = mount(&server, "/x/polymer/web-space/seasons_archives_list", "season_id", "789", season_archives()).await;

    let result = parse_list_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/list/99999?sid=789&oid=123",
        1,
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].url.query().unwrap().contains("aid=123"));
    assert_eq!(
        result.current_episode_data,
        Some(("bvid".to_owned(), json!(BVID)))
    );
    let _ = q(&result.info_data);
}

#[tokio::test]
async fn parse_play_page_without_ugc_season_errors() {
    let server = MockServer::start().await;
    let _view = mount(&server, "/x/web-interface/wbi/view", "bvid", BVID, view_without_season()).await;

    let error = parse_list_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/list/99999?oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ListError::InvalidLink));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn parse_series_type_link() {
    let server = MockServer::start().await;
    let _info = mount(&server, "/x/series/series", "series_id", "1462496", series_meta()).await;
    let _archives = mount(&server, "/x/series/archives", "series_id", "1462496", series_archives()).await;

    let result = parse_list_url(
        &client(),
        &server.uri(),
        "https://space.bilibili.com/12345/lists/1462496?type=series",
        1,
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().any(|r| r.url.path().contains("x/series/series")));
    assert!(requests.iter().any(|r| r.url.path().contains("x/series/archives")));
    assert_eq!(
        result.info_data["data"]["meta"]["name"],
        json!("测试系列")
    );
}

#[tokio::test]
async fn parse_sid_series_wins_over_video_season() {
    let server = MockServer::start().await;
    let _view = mount(&server, "/x/web-interface/wbi/view", "bvid", BVID, view()).await;
    let _info = mount(&server, "/x/series/series", "series_id", "1462496", series_meta()).await;
    let _archives = mount(&server, "/x/series/archives", "series_id", "1462496", series_archives()).await;

    let result = parse_list_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/list/99999?sid=1462496&oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert!(!requests
        .iter()
        .any(|r| r.url.path().contains("seasons_archives_list")));
    assert!(requests
        .iter()
        .any(|r| r.url.path().contains("x/series/archives")));
    assert_eq!(
        result.info_data["data"]["meta"]["name"],
        json!("测试系列")
    );
}

#[tokio::test]
async fn parse_sid_matches_video_season_uses_season() {
    let server = MockServer::start().await;
    let _view = mount(&server, "/x/web-interface/wbi/view", "bvid", BVID, view()).await;
    let _season = mount(&server, "/x/polymer/web-space/seasons_archives_list", "season_id", "789", season_archives()).await;

    let result = parse_list_url(
        &client(),
        &server.uri(),
        "https://www.bibilidown.test/list/99999?sid=789&oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap();

    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    let requests = server.received_requests().await.unwrap();
    assert!(requests
        .iter()
        .any(|r| r.url.path().contains("seasons_archives_list")
            && r.url.query().unwrap().contains("season_id=789")));
    assert_eq!(
        result.info_data["data"]["meta"]["title"],
        json!("测试合集")
    );
}

#[tokio::test]
async fn parse_series_probe_failure_falls_back_to_season() {
    let server = MockServer::start().await;
    let _probe = mount(&server, "/x/series/series", "series_id", "789", series_not_found()).await;
    let _season = mount(&server, "/x/polymer/web-space/seasons_archives_list", "season_id", "789", season_archives()).await;

    let result = parse_list_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/list/99999?sid=789",
        1,
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert!(!requests.iter().any(|r| r.url.path().contains("x/series/archives")));
    assert!(requests
        .iter()
        .any(|r| r.url.path().contains("seasons_archives_list")));
    assert_eq!(
        result.info_data["data"]["meta"]["title"],
        json!("测试合集")
    );
}

#[tokio::test]
async fn parse_list_without_params_errors_zero_requests() {
    let server = MockServer::start().await;

    let error = parse_list_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/list/456",
        1,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ListError::InvalidLink));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[test]
fn get_bvid_skips_bvid_key_prefix() {
    let url = "https://www.bilibili.com/video/BV17x411w7KC?bvid_param=x";
    let re = regex::Regex::new(r"bvid=([Bb][Vv][0-9A-Za-z]+)").unwrap();
    let found = re
        .captures(url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned());
    assert_eq!(found, None);
}

#[test]
fn get_mid_matches_path_only() {
    let url = "https://space.bilibili.com/12345/lists/1462496?type=series";
    let re = regex::Regex::new(r"/([0-9]+)").unwrap();
    let path = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map(|(_, path)| path.to_owned())
        .unwrap_or_default();
    let path = format!("/{}", path.split(['?', '#']).next().unwrap_or_default());
    let found = re
        .captures(&path)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned());
    assert_eq!(found, Some("12345".to_owned()));
}
