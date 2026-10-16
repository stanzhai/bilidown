//! Wiremock tests for the list-family parsers, porting the vectors of
//! `test/test_list_parser.py` (collection dispatch), the uname-cache
//! assertions of `test/test_space_parser.py`, and the request-shape
//! contract of the other families (params, page sizes, keyword param
//! names, paging counts, server-search flags).

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use parsers::list::{
    parse_favlist, parse_history, parse_list, parse_popular, parse_space, parse_watch_later,
    swap_host, ListError, ListParseContext,
};
use parsers::tree::Attribute;

const BVID: &str = "BV1xx411c7mD";

fn archive() -> Value {
    json!({
        "aid": 123,
        "bvid": BVID,
        "pic": "https://example.com/cover.jpg",
        "pubdate": 1700000000,
        "title": "测试视频",
        "duration": 100,
    })
}

fn view() -> Value {
    json!({
        "code": 0,
        "data": {
            "bvid": BVID,
            "aid": 123,
            "cid": 456,
            "ugc_season": {"id": 789, "mid": 99999, "title": "测试合集"},
        }
    })
}

fn view_without_season() -> Value {
    json!({"code": 0, "data": {"bvid": BVID, "aid": 123, "cid": 456}})
}

fn season_archives() -> Value {
    json!({
        "code": 0,
        "data": {
            "meta": {"season_id": 789, "mid": 99999, "title": "测试合集", "name": "合集·测试合集", "total": 1},
            "page": {"page_num": 1, "page_size": 30, "total": 1},
            "archives": [archive()],
        }
    })
}

fn series_meta() -> Value {
    json!({"code": 0, "data": {"meta": {"series_id": 1462496, "mid": 12345, "name": "测试系列", "total": 1}}})
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
            "archives": [archive()],
        }
    })
}

fn mock_client() -> network::HttpClient {
    network::HttpClient::new(network::ClientOptions {
        user_agent: String::new(),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

async fn mount_json(server: &MockServer, path_pattern: &str, body: Value, times: usize) {
    Mock::given(method("GET"))
        .and(path(path_pattern))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(times as u64)
        .mount(server)
        .await;
}

// ---------------------------------------------------------------------------
// Collection / series dispatch (test_list_parser.py)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn play_page_takes_season_from_video() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/web-interface/wbi/view", view(), 1).await;
    mount_json(&server, "/x/polymer/web-space/seasons_archives_list", season_archives(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let output = parse_list(
        &client,
        &server.uri(),
        &ctx,
        "https://www.bilibili.com/list/12345/?oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap();

    // 请求形状：view 用 bvid；seasons_archives_list 的 mid 与 season_id
    // 都以接口返回为准，而不是链接路径里的 12345
    let view_req = &server.received_requests().await.unwrap()[0];
    assert!(format!("{:?}", view_req.url).contains("bvid="));
    let season_req = &server.received_requests().await.unwrap()[1];
    let season_url = season_req.url.to_string();
    assert!(season_url.contains("season_id=789"), "{season_url}");
    assert!(season_url.contains("mid=99999"), "mid 应取接口返回的 {season_url}");

    // 节点结构：wrapper → 合集节点 → 条目；链接指向的视频交给定位
    assert_eq!(output.category, "合集");
    assert_eq!(output.node.child(0).title, "测试合集");
    assert_eq!(
        output.current_episode,
        Some(("bvid".to_owned(), BVID.to_owned())),
        "链接指向的视频应交给解析列表定位"
    );
    assert_eq!(
        output.node.child(0).child(0).bvid, BVID,
        "定位用的字段必须真的存在于条目上"
    );
    assert!(output.node.child(0).child(0).has_attribute(Attribute::NEED_PARSE_BIT));
}

#[tokio::test]
async fn play_page_by_oid_only() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/web-interface/wbi/view", view(), 1).await;
    mount_json(&server, "/x/polymer/web-space/seasons_archives_list", season_archives(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_list(&client, &server.uri(), &ctx, "https://www.bilibili.com/list/12345?oid=123", 1)
        .await
        .unwrap();

    // 少数链接只有 oid，同样按 aid 去查，不另做 av→BV 换算
    let first = &server.received_requests().await.unwrap()[0];
    assert!(first.url.to_string().contains("aid=123"));
}

#[tokio::test]
async fn play_page_video_without_season_raises() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/web-interface/wbi/view", view_without_season(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let error = parse_list(
        &client,
        &server.uri(),
        &ctx,
        "https://www.bilibili.com/list/12345?oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ListError::NoListInLink));
    server.verify().await;
}

#[tokio::test]
async fn play_page_sid_series() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/series/series", series_meta(), 1).await;
    mount_json(&server, "/x/series/archives", series_archives(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_list(&client, &server.uri(), &ctx, "https://www.bilibili.com/list/12345?sid=1462496", 1)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    // 探针拿到的 meta 应被复用（系列元信息接口只打了一次）
    let series_meta_calls = requests
        .iter()
        .filter(|r| r.url.path().contains("x/series/series"))
        .count();
    assert_eq!(series_meta_calls, 1, "探针拿到的 meta 应被复用");
    let archives = requests
        .iter()
        .find(|r| r.url.path().contains("x/series/archives"))
        .map(|r| r.url.to_string())
        .unwrap();
    assert!(archives.contains("series_id=1462496"), "{archives}");
    assert!(
        !requests.iter().any(|r| r.url.path().contains("seasons_archives_list")),
        "探针命中系列后不应再查合集"
    );
}

#[tokio::test]
async fn play_page_sid_beats_the_season_of_the_video() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/web-interface/wbi/view", view(), 1).await;
    mount_json(&server, "/x/series/series", series_meta(), 1).await;
    mount_json(&server, "/x/series/archives", series_archives(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_list(
        &client,
        &server.uri(),
        &ctx,
        "https://www.bilibili.com/list/12345?sid=1462496&oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert!(
        !requests.iter().any(|r| r.url.path().contains("seasons_archives_list")),
        "sid 明说了是系列，不该解析成视频的合集"
    );
    let archives = requests
        .iter()
        .find(|r| r.url.path().contains("x/series/archives"))
        .map(|r| r.url.to_string())
        .unwrap();
    assert!(archives.contains("series_id=1462496"), "{archives}");
}

#[tokio::test]
async fn play_page_sid_matching_the_video_season_skips_the_probe() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/web-interface/wbi/view", view(), 1).await;
    mount_json(&server, "/x/polymer/web-space/seasons_archives_list", season_archives(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_list(
        &client,
        &server.uri(),
        &ctx,
        "https://www.bilibili.com/list/12345?sid=789&oid=123&bvid=BV1xx411c7mD",
        1,
    )
    .await
    .unwrap();

    // 能靠视频判断就不该多打一次探针：总共恰好两个请求
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    let season = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .find(|r| r.url.path().contains("seasons_archives_list"))
        .map(|r| r.url.to_string())
        .unwrap();
    assert!(season.contains("season_id=789"), "{season}");
}

#[tokio::test]
async fn play_page_sid_season() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/series/series", series_not_found(), 1).await;
    mount_json(&server, "/x/polymer/web-space/seasons_archives_list", season_archives(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_list(&client, &server.uri(), &ctx, "https://www.bilibili.com/list/12345?sid=789", 1)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let season = requests
        .iter()
        .find(|r| r.url.path().contains("seasons_archives_list"))
        .map(|r| r.url.to_string())
        .unwrap();
    assert!(season.contains("season_id=789"), "{season}");
    assert!(
        !requests.iter().any(|r| r.url.path().contains("x/series/archives")),
        "探针未命中时不应查系列归档"
    );
}

#[tokio::test]
async fn play_page_without_list_raises() {
    // 合集索引页（没有 sid / oid / bvid）仍然解析不了，且不应发出请求
    let server = MockServer::start().await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let error = parse_list(&client, &server.uri(), &ctx, "https://www.bilibili.com/list/456", 1)
        .await
        .unwrap_err();

    assert!(matches!(error, ListError::InvalidLink));
    assert!(server.received_requests().await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Space (test_space_parser.py cache semantics + params)
// ---------------------------------------------------------------------------

fn space_response(count: i64) -> Value {
    json!({
        "code": 0,
        "data": {
            "list": {"vlist": [
                {"aid": 1, "bvid": "BVspace1", "pic": "p1", "title": "投稿一", "created": 1700000000, "length": "10:30"},
            ]},
            "page": {"pn": 1, "ps": 40, "count": count},
        }
    })
}

fn card_response(name: &str) -> Value {
    json!({"code": 0, "data": {"card": {"name": name}}})
}

#[tokio::test]
async fn space_parses_and_requests_uname_once() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/space/wbi/arc/search", space_response(80), 2).await;
    mount_json(&server, "/x/web-interface/card", card_response("测试UP主"), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let first = parse_space(&client, &server.uri(), &ctx, "https://space.bilibili.com/123", 1)
        .await
        .unwrap();
    let second = parse_space(&client, &server.uri(), &ctx, "https://space.bilibili.com/123", 2)
        .await
        .unwrap();

    // 缓存命中：翻页不再请求用户名接口（test_space_parser 的守门用例）
    let card_calls = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().contains("web-interface/card"))
        .count();
    assert_eq!(card_calls, 1, "缓存命中后仍然发起了请求，return 丢失");

    assert_eq!(first.node.child(0).title, "测试UP主");
    assert_eq!(first.extra.pagination.as_ref().unwrap().total_pages, 2);
    assert_eq!(first.extra.pagination.as_ref().unwrap().total_items, 80);
    assert!(first.extra.server_search);
    let item = first.node.child(0).child(0);
    assert_eq!(item.bvid, "BVspace1");
    assert_eq!(item.duration, 630, "MM:SS 文本时长要换算成秒");
    assert!(item.has_attribute(Attribute::SPACE_BIT | Attribute::NEED_PARSE_BIT));
    assert_eq!(second.extra.pagination.as_ref().unwrap().current_page, 2);
}

#[tokio::test]
async fn space_different_mid_requests_again() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/space/wbi/arc/search", space_response(1), 2).await;
    mount_json(&server, "/x/web-interface/card", card_response("A"), 2).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_space(&client, &server.uri(), &ctx, "https://space.bilibili.com/123", 1).await.unwrap();
    parse_space(&client, &server.uri(), &ctx, "https://space.bilibili.com/456", 1).await.unwrap();

    let card_calls = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().contains("web-interface/card"))
        .count();
    assert_eq!(card_calls, 2);
}

#[tokio::test]
async fn space_search_keyword_flows_to_params_and_title() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/space/wbi/arc/search", space_response(1), 1).await;
    mount_json(&server, "/x/web-interface/card", card_response("UP"), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let output = parse_space(
        &client,
        &server.uri(),
        &ctx,
        "https://space.bilibili.com/123?keyword=%E6%B5%8B%E8%AF%95",
        1,
    )
    .await
    .unwrap();

    let arc = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .find(|r| r.url.path().contains("arc/search"))
        .map(|r| r.url.to_string())
        .unwrap();
    assert!(arc.contains("keyword="), "{arc}");
    assert_eq!(output.node.child(0).title, "UP - 搜索: 测试");
    assert_eq!(output.extra.keyword, "测试");
}

#[tokio::test]
async fn space_mid_ignores_query_numbers() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/space/wbi/arc/search", space_response(1), 1).await;
    mount_json(&server, "/x/web-interface/card", card_response("UP"), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_space(&client, &server.uri(), &ctx, "https://space.bilibili.com/123?pn=99", 1)
        .await
        .unwrap();

    let card = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .find(|r| r.url.path().contains("web-interface/card"))
        .map(|r| r.url.to_string())
        .unwrap();
    assert!(card.contains("mid=123"), "mid 不应被查询串里的数字干扰 {card}");
}

// ---------------------------------------------------------------------------
// Favorites
// ---------------------------------------------------------------------------

fn favlist_response(media_count: i64) -> Value {
    json!({
        "code": 0,
        "data": {
            "info": {"id": 7, "title": "我的收藏", "media_count": media_count, "upper": {"mid": 123, "name": "UP"}},
            "medias": [
                {"id": 11, "bvid": "BVfav1", "title": "收藏一", "cover": "c1", "duration": 60, "pubtime": 1700000000, "fav_time": 1700000001, "page": 1},
                {"id": 12, "bvid": "BVfav2", "title": "多P", "intro": "x", "cover": "c2", "duration": 61, "pubtime": 1700000000, "fav_time": 1700000001, "page": 3},
            ],
        }
    })
}

#[tokio::test]
async fn favlist_parses_params_and_badges() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/v3/fav/resource/list", favlist_response(80), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let output = parse_favlist(
        &client,
        &server.uri(),
        &ctx,
        "https://space.bilibili.com/123/favlist?fid=456",
        1,
    )
    .await
    .unwrap();

    let url = server.received_requests().await.unwrap()[0].url.to_string();
    assert!(url.contains("media_id=456"), "{url}");
    assert!(url.contains("ps=40"), "{url}");
    assert!(url.contains("keyword="), "{url}");
    assert!(url.contains("order=mtime"), "{url}");
    assert!(url.contains("web_location=333.1387"), "{url}");
    assert!(!url.contains("w_rid"), "收藏夹接口不应带 wbi 签名 {url}");

    assert_eq!(output.category, "收藏夹");
    assert_eq!(output.node.child(0).title, "我的收藏");
    assert_eq!(output.extra.pagination.as_ref().unwrap().total_pages, 2);
    assert!(output.extra.server_search);

    let items = &output.node.child(0);
    assert_eq!(items.child(0).bvid, "BVfav1");
    assert!(items.child(0).has_attribute(Attribute::FAVLIST_BIT | Attribute::NEED_PARSE_BIT | Attribute::VIDEO_BIT));
    // page > 1 的条目：分P徽章 + FAVORITE_WITH_MULTI_PART_VIDEO_BIT
    assert_eq!(items.child(1).badge, "分P");
    assert!(items.child(1).has_attribute(Attribute::FAVORITE_WITH_MULTI_PART_VIDEO_BIT));
    assert_eq!(items.child(1).favtime, 1700000001);
}

#[tokio::test]
async fn favlist_ogv_item_is_bangumi_shaped() {
    let body = json!({
        "code": 0,
        "data": {
            "info": {"id": 7, "title": "t", "media_count": 1, "upper": {"mid": 1, "name": "u"}},
            "medias": [
                {"id": 11, "cover": "c", "duration": 60, "pubtime": 1, "fav_time": 2, "page": 1,
                 "ogv": {"title": "某番剧", "intro": "第一季", "type_name": "番剧"}},
            ],
        }
    });
    let server = MockServer::start().await;
    mount_json(&server, "/x/v3/fav/resource/list", body, 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let output = parse_favlist(&client, &server.uri(), &ctx, "https://space.bilibili.com/123/favlist?fid=7", 1)
        .await
        .unwrap();

    let item = output.node.child(0).child(0);
    assert!(item.has_attribute(Attribute::BANGUMI_BIT));
    assert_eq!(item.badge, "番剧");
    assert_eq!(item.title, "某番剧 - 第一季");
}

#[tokio::test]
async fn favlist_ml_path_id_is_accepted() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/v3/fav/resource/list", favlist_response(1), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    parse_favlist(&client, &server.uri(), &ctx, "https://space.bilibili.com/123/favlist?fid=456&mlid=789", 1)
        .await
        .unwrap();

    let url = server.received_requests().await.unwrap()[0].url.to_string();
    // fid= 优先于 ml 提取（与 Python 相同的 find_str 顺序）
    assert!(url.contains("media_id=456"), "{url}");
}

// ---------------------------------------------------------------------------
// Watch later
// ---------------------------------------------------------------------------

fn watch_later_response(count: i64) -> Value {
    json!({
        "code": 0,
        "data": {
            "count": count,
            "list": [
                {"aid": 1, "bvid": "BVwl1", "cid": 10, "pic": "c", "title": "稍后一", "duration": 100, "pubdate": 1700000000, "add_at": 1700000100},
                {"aid": 2, "bvid": "BVwl2", "cid": 11, "pic": "c", "title": "番剧稍后", "duration": 200, "pubdate": 1700000000, "add_at": 1700000100,
                 "pgc_label": "国创", "bangumi": {"ep_id": 777}},
            ],
        }
    })
}

fn logged_in_auth() -> auth::AuthState {
    auth::AuthState { is_login: true, ..auth::AuthState::default() }
}

#[tokio::test]
async fn watch_later_requires_login() {
    let server = MockServer::start().await;
    let client = mock_client();
    let ctx = ListParseContext::new();
    let anonymous = auth::AuthState::default();

    let error = parse_watch_later(
        &client,
        &server.uri(),
        &ctx,
        "bili23://watch_later",
        &anonymous,
        1,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ListError::LoginRequired));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn watch_later_uses_key_param_and_maps_bangumi() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/v2/history/toview/web", watch_later_response(40), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();
    let auth_state = logged_in_auth();

    let output = parse_watch_later(
        &client,
        &server.uri(),
        &ctx,
        "bili23://watch_later?key=%E6%B5%8B%E8%AF%95",
        &auth_state,
        1,
    )
    .await
    .unwrap();

    let url = server.received_requests().await.unwrap()[0].url.to_string();
    // 稍后再看接口的关键词参数名为 key，与其他接口的 keyword 不同
    assert!(url.contains("key="), "{url}");
    assert!(!url.contains("keyword="), "{url}");
    assert!(url.contains("ps=20"), "{url}");
    assert!(url.contains("viewed=0"), "{url}");
    assert!(url.contains("asc=false"), "{url}");

    assert_eq!(output.extra.pagination.as_ref().unwrap().total_pages, 2);
    assert_eq!(output.node.child(0).title, "搜索: 测试");

    let items = &output.node.child(0);
    assert!(items.child(0).has_attribute(Attribute::WATCH_LATER_BIT | Attribute::NEED_PARSE_BIT | Attribute::VIDEO_BIT));
    let bangumi = items.child(1);
    assert!(bangumi.has_attribute(Attribute::BANGUMI_BIT));
    assert_eq!(bangumi.badge, "国创");
    assert_eq!(bangumi.ep_id, 777);
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

fn history_response(total: i64) -> Value {
    json!({
        "code": 0,
        "data": {
            "page": {"pn": 1, "ps": 20, "total": total},
            "list": [
                {"title": "看过一", "cover": "c", "uri": "", "view_at": 1700000100, "duration": 100, "badge": "",
                 "history": {"bvid": "BVhis1", "cid": 20, "epid": 0, "business": "archive"}},
                {"title": "番剧历史", "show_title": "第1话", "cover": "c", "uri": "bilibili://bangumi/ep/789", "view_at": 1700000100, "duration": 200, "badge": "",
                 "history": {"bvid": "BVhis2", "cid": 21, "epid": 789, "business": "pgc"}},
                {"title": "失效视频", "cover": "c", "uri": "", "view_at": 1700000100, "duration": 0, "badge": "",
                 "history": {"bvid": "BVhis3", "cid": 22, "epid": 0, "business": "archive"}},
                {"title": "课程历史", "cover": "c", "uri": "", "view_at": 1700000100, "duration": 100, "badge": "",
                 "history": {"bvid": "BVhis4", "cid": 23, "epid": 0, "business": "cheese"}},
            ],
        }
    })
}

#[tokio::test]
async fn history_requires_login() {
    let server = MockServer::start().await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let error = parse_history(
        &client,
        &server.uri(),
        &ctx,
        "bili23://history",
        &auth::AuthState::default(),
        1,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ListError::LoginRequired));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn history_maps_business_shapes_and_expired() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/web-interface/history/search", history_response(40), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();
    let auth_state = logged_in_auth();

    let output = parse_history(
        &client,
        &server.uri(),
        &ctx,
        "bili23://history?keyword=%E6%B5%8B%E8%AF%95",
        &auth_state,
        1,
    )
    .await
    .unwrap();

    let url = server.received_requests().await.unwrap()[0].url.to_string();
    assert!(url.contains("keyword="), "{url}");
    assert!(url.contains("business=archive"), "{url}");
    assert!(url.contains("web_location=333.1391"), "{url}");
    assert!(!url.contains("ps="), "历史接口不发 ps {url}");
    assert!(!url.contains("w_rid"), "历史接口不应带 wbi 签名 {url}");

    assert_eq!(output.extra.pagination.as_ref().unwrap().total_pages, 2);
    assert_eq!(output.extra.pagination.as_ref().unwrap().total_items, 40);

    let items = &output.node.child(0);
    // archive → VIDEO_BIT，且有 uri 时用 uri，否则拼视频链接
    assert!(items.child(0).has_attribute(Attribute::HISTORY_BIT | Attribute::NEED_PARSE_BIT | Attribute::VIDEO_BIT));
    assert_eq!(items.child(0).url, "https://www.bilibili.com/video/BVhis1");
    assert_eq!(items.child(0).viewtime, 1700000100);
    // pgc → BANGUMI_BIT，uri 直传，标题拼 show_title
    assert!(items.child(1).has_attribute(Attribute::BANGUMI_BIT));
    assert_eq!(items.child(1).url, "bilibili://bangumi/ep/789");
    assert_eq!(items.child(1).title, "番剧历史 - 第1话");
    // duration == 0 → 已失效
    assert_eq!(items.child(2).badge, "已失效");
    assert!(items.child(2).expired);
    // cheese → CHEESE_BIT（未知 business 一律落到 VIDEO_BIT，见
    // test_reparse_worker 的失败形态契约）
    assert!(items.child(3).has_attribute(Attribute::CHEESE_BIT));
}

// ---------------------------------------------------------------------------
// Weekly popular
// ---------------------------------------------------------------------------

fn weekly_response() -> Value {
    json!({
        "code": 0,
        "data": {
            "config": {"label": "第 200 期"},
            "list": [
                {"aid": 5, "bvid": "BVweek1", "cid": 50, "pic": "c", "title": "每周一", "duration": 300, "pubdate": 1700000000},
            ],
        }
    })
}

#[tokio::test]
async fn popular_uses_number_param_and_has_no_pagination() {
    let server = MockServer::start().await;
    mount_json(&server, "/x/web-interface/popular/series/one", weekly_response(), 1).await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let output = parse_popular(&client, &server.uri(), &ctx, "https://www.bilibili.com/v/popular/weekly?num=200")
        .await
        .unwrap();

    let url = server.received_requests().await.unwrap()[0].url.to_string();
    assert!(url.contains("number=200"), "{url}");
    assert!(url.contains("web_location=333.934"), "{url}");

    assert_eq!(output.node.child(0).title, "第 200 期");
    assert!(output.extra.pagination.is_none(), "每周必看无分页");
    assert!(!output.extra.server_search);
    let item = output.node.child(0).child(0);
    assert!(item.has_attribute(Attribute::WEEKLY_BIT | Attribute::VIDEO_BIT));
    assert_eq!(item.related_titles.get("source_title").unwrap(), "第 200 期");
}

// ---------------------------------------------------------------------------
// check_response + host swap
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_zero_code_is_an_api_error_with_the_message() {
    let server = MockServer::start().await;
    mount_json(
        &server,
        "/x/v3/fav/resource/list",
        json!({"code": -404, "message": "啥都木有"}),
        1,
    )
    .await;
    let client = mock_client();
    let ctx = ListParseContext::new();

    let error = parse_favlist(&client, &server.uri(), &ctx, "https://space.bilibili.com/1/favlist?fid=1", 1)
        .await
        .unwrap_err();

    assert_eq!(error.to_string(), "啥都木有");
}

#[test]
fn swap_host_keeps_path_and_supports_empty_base() {
    assert_eq!(
        swap_host("https://api.bilibili.com/x/series/series", "http://localhost:1"),
        "http://localhost:1/x/series/series"
    );
    assert_eq!(
        swap_host("https://api.bilibili.com/x/series/series", ""),
        "https://api.bilibili.com/x/series/series"
    );
}
