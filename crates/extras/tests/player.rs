//! `additional/player.py`: player info fetch shapes.

mod common;

use common::*;
use serde_json::json;

#[tokio::test]
async fn sends_fingerprint_params_and_bvid() {
    let server = wiremock::MockServer::start().await;
    mount_json(&server, "/x/player/wbi/v2", json!({"data": {"cid": 99}}), 1).await;

    let mut task = database::TaskInfo::default();
    task.Episode.cid = 99;
    task.Episode.bvid = "BV1xx".to_owned();

    let data = extras::player::get_data(&mock_client(), &server.uri(), &task)
        .await
        .unwrap();
    assert_eq!(data, json!({"cid": 99}));

    let request = &server.received_requests().await.unwrap()[0];
    let query = request.url.query().unwrap_or_default();
    assert!(query.contains("cid=99"));
    assert!(query.contains("bvid=BV1xx"));
    assert!(query.contains("dm_img_str=V2ViR0wgMS4wIChPcGVuR0wgRVMgMi4wIENocm9taXVtKQ"));
    assert!(
        query.contains("dm_img_list=%5B%5D"),
        "dm_img_list=[] must serialize as %5B%5D"
    );
}

#[tokio::test]
async fn course_uses_aid_when_bvid_empty() {
    let server = wiremock::MockServer::start().await;
    mount_json(&server, "/x/player/wbi/v2", json!({}), 1).await;

    let mut task = database::TaskInfo::default();
    task.Episode.cid = 7;
    task.Episode.aid = 12345;
    // bvid 留空 → 用 aid

    let data = extras::player::get_data(&mock_client(), &server.uri(), &task)
        .await
        .unwrap();
    assert_eq!(data, json!({}));

    let requests = server.received_requests().await.unwrap();
    let query = requests[0].url.query().unwrap_or_default().to_owned();
    assert!(query.contains("aid=12345"));
    assert!(!query.contains("bvid="));
}
