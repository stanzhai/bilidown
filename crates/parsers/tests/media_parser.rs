//! Media-link HTTP parser tests (ticket 09 slice 4): video dispatch
//! (av→BV, page override, redirect, interactive BFS), bangumi ep/ss/md,
//! cheese, and mall lesson (login guard, POST body, play payload).

use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use parsers::episode::video::episode_badge;
use parsers::media_parser::*;
use parsers::tree::{Attribute, EpisodeData, Number};

fn client() -> network::HttpClient {
    // `list_parsers`' helper shape: anonymous session, no proxy.
    network::HttpClient::new(network::ClientOptions {
        user_agent: "test".to_owned(),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

fn view_data(bvid: &str, title: &str) -> Value {
    json!({
        "aid": 170001, "bvid": bvid, "cid": 1001, "pic": "cover.jpg",
        "pubdate": 1700000000, "title": title, "duration": 300,
        "desc": "简介", "tid": 21, "tid_v2": 171,
        "is_upower_exclusive": false, "attribute": 0,
        "rights": {"is_stein_gate": 0},
        "owner": {"mid": 42, "name": "UP", "face": "f.jpg"},
        "pages": [{"cid": 1001, "page": 1, "part": title, "duration": 300, "ctime": 1700000000}]
    })
}

async fn mount_view(server: &MockServer, body: Value) {
    Mock::given(method("GET"))
        .and(path("/x/web-interface/wbi/view"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0", "data": body
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn av_link_converts_to_bvid() {
    assert_eq!(
        video_bvid("https://www.bilibili.com/video/av170001"),
        Some("BV17x411w7KC".to_owned())
    );
    assert_eq!(
        video_bvid("https://www.bilibili.com/video/BV1xx411c7mD"),
        Some("BV1xx411c7mD".to_owned())
    );
}

#[tokio::test]
async fn single_video_parse() {
    let server = MockServer::start().await;
    mount_view(&server, view_data("BV1xx411c7mD", "单视频")).await;

    let data = EpisodeData::default();
    let outcome = parse_video_url(&client(), &server.uri(), "https://www.bilibili.com/video/BV1xx411c7mD", &data)
        .await
        .unwrap();

    let ParseOutcome::Episode(node, (key, value)) = outcome else {
        panic!("expected episode outcome");
    };
    assert_eq!(node.child(0).title, "单视频");
    assert_eq!(key, "cid");
    assert_eq!(value, json!(1001));

    // Exactly one request, keyed on bvid.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.query().unwrap().contains("bvid=BV1xx411c7mD"));
}

#[tokio::test]
async fn page_pin_overrides_info_data() {
    let server = MockServer::start().await;
    let mut body = view_data("BV1p", "多P视频");
    body["pages"] = json!([
        {"cid": 2001, "page": 1, "part": "第一P", "duration": 100, "ctime": 1700000100},
        {"cid": 2002, "page": 2, "part": "第二P", "duration": 110, "ctime": 1700000200}
    ]);
    mount_view(&server, body).await;

    let data = get_video_info(
        &client(),
        &server.uri(),
        "BV1p",
        "https://www.bilibili.com/video/BV1p?p=2",
    )
    .await
    .unwrap();

    assert_eq!(data["cid"], json!(2002));
    assert_eq!(data["title"], json!("第二P"));
    assert_eq!(data["duration"], json!(110));
    assert_eq!(data["pubdate"], json!(1700000200));
    assert!(data.get("pages").is_none());
    assert_eq!(data["current_page_number"], json!(2));
}

#[tokio::test]
async fn redirect_link_short_circuits() {
    let server = MockServer::start().await;
    let mut body = view_data("BV1r", "重定向");
    body["redirect_url"] = json!("https://www.bilibili.com/bangumi/play/ep501");
    mount_view(&server, body).await;

    let data = EpisodeData::default();
    let outcome = parse_video_url(&client(), &server.uri(), "https://www.bilibili.com/video/BV1r", &data)
        .await
        .unwrap();
    let ParseOutcome::Redirect(url) = outcome else {
        panic!("expected redirect");
    };
    assert_eq!(url, "https://www.bilibili.com/bangumi/play/ep501");
}

#[tokio::test]
async fn interactive_video_walks_the_graph() {
    let server = MockServer::start().await;
    let mut body = view_data("BV1s", "互动视频");
    body["rights"]["is_stein_gate"] = json!(1);
    mount_view(&server, body.clone()).await;

    // graph_version probe.
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/v2"))
        .and(query_param("aid", "170001"))
        .and(query_param("cid", "1001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {"interaction": {"graph_version": 777}}
        })))
        .mount(&server)
        .await;

    // Root edge → two choices; second edge closes the cycle back to the
    // root's cid so the visited-set dedup must hold.
    Mock::given(method("GET"))
        .and(path("/x/stein/edgeinfo_v2"))
        .and(query_param("edge_id", "0"))
        .and(query_param("graph_version", "777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {"title": "开端", "edges": {"questions": [
                {"type": 1, "choices": [
                    {"id": 1, "option": "走左边", "cid": 2002},
                    {"id": 2, "option": "走右边", "cid": 3003}
                ]}
            ]}}
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/x/stein/edgeinfo_v2"))
        .and(query_param("edge_id", "1"))
        .and(query_param("graph_version", "777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {"story_list": [{"title": "左边结局"}], "edges": {"questions": [
                {"type": 1, "choices": [{"id": 3, "option": "回到开头", "cid": 1001}]}
            ]}}
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/x/stein/edgeinfo_v2"))
        .and(query_param("edge_id", "2"))
        .and(query_param("graph_version", "777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {"title": "右边结局", "edges": {}}
        })))
        .mount(&server)
        .await;

    // The cycle back-edge (1001, edge_id=3) is fetched too — same node,
    // different edge state — and reports no further choices.
    Mock::given(method("GET"))
        .and(path("/x/stein/edgeinfo_v2"))
        .and(query_param("edge_id", "3"))
        .and(query_param("graph_version", "777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {"title": "开端", "edges": {}}
        })))
        .mount(&server)
        .await;

    let episode_data = EpisodeData::default();
    let tree = parse_interactive(&client(), &server.uri(), &body, &episode_data)
        .await
        .unwrap();

    // Root + 3 visited nodes (start, left end, right end); the
    // (1001, 3) back-edge state is new so the cycle revisits cid 1001
    // once more — but with no options it adds nothing beyond the title
    // refresh. Visited states: (1001,0), (2002,1), (3003,2), (1001,3).
    let titles: Vec<&str> = tree.children.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles.len(), 4);
    assert!(titles.contains(&"开端"));
    assert!(titles.contains(&"左边结局"));
    assert!(titles.contains(&"右边结局"));

    let first = &tree.children[0];
    assert!(first.has_attribute(Attribute::INTERACTIVE_BIT | Attribute::VIDEO_BIT));
    assert_eq!(first.cid, 1001);
    assert_eq!(first.number, Number::Int(1));
    assert_eq!(first.related_titles["collection_title"], "互动视频");
    assert_eq!(first.url, "https://www.bilibili.com/video/BV1s");
}

#[tokio::test]
async fn interactive_without_graph_version_errors() {
    let server = MockServer::start().await;
    let mut body = view_data("BV1n", "假互动");
    body["rights"]["is_stein_gate"] = json!(1);
    mount_view(&server, body.clone()).await;

    Mock::given(method("GET"))
        .and(path("/x/player/wbi/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0", "data": {}
        })))
        .mount(&server)
        .await;

    let err = parse_interactive(&client(), &server.uri(), &body, &EpisodeData::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("graph_version"));
}

#[tokio::test]
async fn bangumi_ep_link_marks_current() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pgc/view/web/season"))
        .and(query_param("ep_id", "501"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "result": {
                "season_id": 431, "media_id": 2823, "season_title": "测试番剧",
                "series": {"series_title": "系列"},
                "seasons": [{"season_id": 431}],
                "areas": [{"name": "日本"}],
                "publish": {"pub_time": "2023-05-01 00:00:00"},
                "evaluate": "简介", "styles": [], "cover": "p.jpg", "actors": "",
                "episodes": [
                    {"aid": 11, "bvid": "BV1e1", "cid": 111, "ep_id": 501, "badge": "",
                     "cover": "e.jpg", "duration": 1440000,
                     "link": "https://www.bilibili.com/bangumi/play/ep501",
                     "pub_time": 1682870400, "show_title": "第一话", "title": "第1话"}
                ],
                "section": []
            }
        })))
        .mount(&server)
        .await;

    let data = EpisodeData::default();
    let node = parse_bangumi_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/bangumi/play/ep501",
        &data,
    )
    .await
    .unwrap();

    assert_eq!(node.title, "测试番剧");
    assert_eq!(node.child(0).child(0).title, "第一话");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.query().unwrap().contains("ep_id=501"));
}

#[tokio::test]
async fn bangumi_md_link_resolves_through_review() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pgc/review/user"))
        .and(query_param("media_id", "2823"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "result": {"media": {"season_id": 431}}
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/pgc/view/web/season"))
        .and(query_param("season_id", "431"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "result": {
                "season_id": 431, "season_title": "md番剧",
                "series": {"series_title": "系列"}, "seasons": [],
                "areas": [], "publish": {"pub_time": ""},
                "evaluate": "", "styles": [], "cover": "", "actors": "",
                "episodes": [], "section": []
            }
        })))
        .mount(&server)
        .await;

    let node = parse_bangumi_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/bangumi/media/md2823",
        &EpisodeData::default(),
    )
    .await
    .unwrap();
    assert_eq!(node.title, "md番剧");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].url.query().unwrap().contains("season_id=431"));
}

#[tokio::test]
async fn cheese_ss_link_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pugv/view/web/season/v2"))
        .and(query_param("season_id", "888"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {
                "title": "测试课程", "season_id": 888, "cover": "c.jpg",
                "subtitle": "副标题", "release_date": 1682870400,
                "up_info": {"uname": "UP", "mid": 66, "avatar": "a.jpg"},
                "sections": [
                    {"title": "第1季", "episodes": [
                        {"aid": 21, "cid": 211, "cover": "e.jpg", "duration": 300,
                         "id": 3001, "play_way_subtitle": "正片", "subtitle": "副",
                         "release_date": 1682870401, "status": 2, "title": "第1集"}
                    ]}
                ]
            }
        })))
        .mount(&server)
        .await;

    let node = parse_cheese_url(
        &client(),
        &server.uri(),
        "https://www.bilibili.com/cheese/play/ss888",
        &EpisodeData::default(),
    )
    .await
    .unwrap();

    assert_eq!(node.title, "测试课程");
    assert_eq!(node.child(0).child(0).badge, "付费");
}

#[tokio::test]
async fn lesson_requires_login_and_valid_ids() {
    let server = MockServer::start().await;
    let anonymous = auth::AuthState::default();

    let err = parse_lesson_url(
        &client(),
        &server.uri(),
        "https://mall.bilibili.com/lesson/play?courseId=1&lessonId=2&itemId=3",
        &anonymous,
        &EpisodeData::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, MediaError::LoginRequired));
    assert!(server.received_requests().await.unwrap().is_empty());

    let logged_in = auth::AuthState { is_login: true, ..Default::default() };
    let err = parse_lesson_url(
        &client(),
        &server.uri(),
        "https://mall.bilibili.com/lesson/play?courseId=1&lessonId=2",
        &logged_in,
        &EpisodeData::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, MediaError::InvalidLink));
}

#[tokio::test]
async fn lesson_post_builds_tree_and_play_payload() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mall-search-items/items/course/h5/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0",
            "data": {
                "lessonName": "商城课程", "itemsName": "【营销】商城课程",
                "chapterList": [
                    {"chapterName": "", "sectionList": [
                        {"sectionId": 5001, "sectionName": "第一课", "videoTime": 90500,
                         "sectionIndex": 1, "hasWatchRight": true}
                    ]}
                ]
            }
        })))
        .mount(&server)
        .await;

    let logged_in = auth::AuthState { is_login: true, ..Default::default() };
    let node = parse_lesson_url(
        &client(),
        &server.uri(),
        "https://mall.bilibili.com/lesson/play?courseId=100&lessonId=200&itemId=300",
        &logged_in,
        &EpisodeData::default(),
    )
    .await
    .unwrap();

    assert_eq!(node.title, "商城课程");
    let item = node.child(0);
    assert_eq!(item.section_id, 5001);
    assert_eq!(item.duration, 90);
    assert_eq!(
        item.url,
        "https://mall.bilibili.com/lesson/play?courseId=100&lessonId=200&itemId=300"
    );

    // POST body carried the ids.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value =
        serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body, json!({"courseId": 100, "lessonId": 200, "itemId": 300}));

    // Play payload wrapper.
    let media = build_lesson_media_info(&json!({
        "videoUrl": "https://cdn.example.com/lesson.mp4", "videoTime": 90
    }))
    .unwrap();
    assert_eq!(media["quality"], json!(80));
    assert_eq!(media["durl"][0]["url"], json!("https://cdn.example.com/lesson.mp4"));
    assert_eq!(media["durl"][0]["length"], json!(90000));

    assert!(build_lesson_media_info(&json!({})).is_err());
    assert_eq!(
        build_lesson_play_payload(1, 2, 3, 4),
        json!({"courseId": 1, "lessonId": 2, "itemId": 3, "sectionId": 4})
    );
}

#[tokio::test]
async fn lesson_missing_data_body_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mall-search-items/items/course/h5/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "message": "0", "data": null
        })))
        .mount(&server)
        .await;

    let logged_in = auth::AuthState { is_login: true, ..Default::default() };
    let err = parse_lesson_url(
        &client(),
        &server.uri(),
        "https://mall.bilibili.com/lesson/play?courseId=1&lessonId=2&itemId=3",
        &logged_in,
        &EpisodeData::default(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("请确认该课程是否已购买"));
}

#[test]
fn badge_map_bits() {
    assert_eq!(episode_badge(0), "");
    assert_eq!(episode_badge(8), "充电专属"); // 1<<3
    assert_eq!(episode_badge(1 << 3 | 1), "充电专属");
}
