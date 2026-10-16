//! Tool-layer integration tests at the JSON-RPC seam: real parse against a
//! wiremock view endpoint, task listing over a tmp-dir TaskManager, and the
//! create_download duplicate path.

use std::sync::Arc;

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use mcp::state::McpState;
use mcp::tools::{download, parse, task};

fn state(tmp: &tempfile::TempDir, api_base: &str) -> Arc<McpState> {
    let mut store = config::ConfigStore::with_paths(
        tmp.path().join("config"),
        tmp.path().join("downloads"),
    );
    store.set("Behavior", "mcp_enabled", json!(true));
    let store = Arc::new(parking_lot::Mutex::new(store));
    let manager = Arc::new(tasks::TaskManager::open(
        tmp.path().join("task.db"),
        {
            // TaskManager::open takes a shared ConfigStore, not the MCP
            // mutex wrapper — build a second instance from the same paths.
            let shared = config::ConfigStore::with_paths(
                tmp.path().join("config"),
                tmp.path().join("downloads"),
            );
            std::sync::Arc::new(shared)
        },
    ).unwrap());
    Arc::new(McpState::new(store, manager, api_base))
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

// A DASH playurl body (playurl_preview.rs's shape) so the preview flow
// reports available qualities.
fn dash_body() -> Value {
    json!({
        "code": 0, "message": "0", "ttl": 1,
        "data": {
            "quality": 80, "accept_quality": [80, 64], "timelength": 120000,
            "support_formats": [
                {"quality": 80, "codecs": ["avc1.64003e", "hev1.1.6.L120.90"]},
                {"quality": 64, "codecs": ["avc1.64001f"]}
            ],
            "dash": {
                "duration": 120,
                "video": [
                    {"id": 80, "codecid": 7, "baseUrl": "https://cdn/v80-avc.m4s",
                     "backupUrl": [], "bandwidth": 2000000, "mimeType": "video/mp4",
                     "codecs": "avc1.64003e", "frame_rate": "25"},
                    {"id": 64, "codecid": 7, "baseUrl": "https://cdn/v64-avc.m4s",
                     "backupUrl": [], "bandwidth": 900000, "mimeType": "video/mp4",
                     "codecs": "avc1.64001f", "frame_rate": "25"}
                ],
                "audio": [
                    {"id": 30280, "baseUrl": "https://cdn/a192.m4s", "backupUrl": [],
                     "bandwidth": 320000, "mimeType": "audio/mp4", "codecs": "mp4a.40.2"}
                ],
                "flac": null, "dolby": {"type": 0, "audio": null}
            }
        }
    })
}

async fn mount_playurl(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .respond_with(ResponseTemplate::new(200).set_body_json(dash_body()))
        .mount(server)
        .await;
}

fn args(value: Value) -> serde_json::Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[tokio::test]
async fn parse_url_lists_episodes_and_reports_media_info() {
    let server = MockServer::start().await;
    mount_view(&server, view_data("BV1xx411c7mD", "单视频")).await;
    mount_playurl(&server).await;

    let tmp = tempfile::tempdir().unwrap();
    let state = state(&tmp, &server.uri());

    let output = parse::tool_parse_url(
        state.clone(),
        &args(json!({"url": "https://www.bilibili.com/video/BV1xx411c7mD"})),
    )
    .await;

    assert!(!output.is_error, "summary: {}", output.text);
    let structured = output.structured.as_ref().unwrap();
    assert_eq!(structured["total"], json!(1));
    assert_eq!(structured["category"], json!("User Uploads"));
    assert_eq!(
        structured["episodes"][0]["title"],
        json!("单视频")
    );
    // The preview flow ran against the mock playurl.
    assert_eq!(structured["available"]["video_quality"], json!(["auto", "1080P", "720P"]));

    // get_episodes sees the same position id.
    let episodes = parse::tool_get_episodes(state.clone(), &args(json!({})));
    assert!(!episodes.is_error);
    assert_eq!(episodes.structured.as_ref().unwrap()["total"], json!(1));
}

#[tokio::test]
async fn create_download_reports_missing_and_duplicate_paths() {
    let server = MockServer::start().await;
    mount_view(&server, view_data("BV1xx411c7mD", "单视频")).await;
    mount_playurl(&server).await;

    let tmp = tempfile::tempdir().unwrap();
    let state = state(&tmp, &server.uri());

    // Parse first so the position ids exist.
    parse::tool_parse_url(
        state.clone(),
        &args(json!({"url": "https://www.bilibili.com/video/BV1xx411c7mD"})),
    )
    .await;

    // Unknown id: rejected with the not-found detail.
    let missing = download::dispatch(
        state.clone(),
        "create_download",
        &args(json!({"episode_ids": ["999"]})),
    );
    assert!(missing.is_error);
    assert!(
        missing.text.contains("No downloadable episodes matched"),
        "got: {}",
        missing.text
    );

    // Seed an existing completed row so episode 1 counts as duplicate.
    {
        let dict = {
            let view = state.view.lock();
            view.get_all_items()[0].to_dict()
        };
        let map = dict.as_object().cloned().unwrap();
        let mut db = state.manager.reader().unwrap();
        let mut record = database::TaskDatabase::build_record(&database::TaskInfo::default(), false);
        record.hash_id = tasks::duplicate::calc_hash_id_from_episode(&map);
        db.add_task_records(std::slice::from_ref(&record), false).unwrap();
    }

    let duplicate = download::dispatch(
        state.clone(),
        "create_download",
        &args(json!({"episode_ids": ["1"]})),
    );
    assert!(!duplicate.is_error);
    assert!(duplicate.text.contains("Nothing to download"), "got: {}", duplicate.text);
    assert_eq!(duplicate.structured.as_ref().unwrap()["created"], json!(0));

    // redownload=true overrides and creates the task.
    let redownload = download::dispatch(
        state.clone(),
        "create_download",
        &args(json!({"episode_ids": ["1"], "redownload": true})),
    );
    assert!(!redownload.is_error, "got: {}", redownload.text);
    assert_eq!(redownload.structured.as_ref().unwrap()["created"], json!(1), "summary: {}", redownload.text);

    // The task shows up in list_tasks.
    let listed = task::dispatch(state.clone(), "list_tasks", &args(json!({})));
    assert!(!listed.is_error);
    assert_eq!(listed.structured.as_ref().unwrap()["total"], json!(2), "summary: {}", listed.text);
}
