//! Shared helpers: task fixtures, wiremock mounts and config stores.

#![allow(dead_code)]

use chrono::{Datelike, TimeZone};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub fn mock_client() -> network::HttpClient {
    network::HttpClient::new(network::ClientOptions {
        user_agent: String::new(),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

pub async fn mount_json(server: &MockServer, path_pattern: &str, body: Value, times: usize) {
    Mock::given(method("GET"))
        .and(path(path_pattern))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(times as u64)
        .mount(server)
        .await;
}

pub async fn mount_bytes(server: &MockServer, path_pattern: &str, body: Vec<u8>, times: usize) {
    Mock::given(method("GET"))
        .and(path(path_pattern))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .expect(times as u64)
        .mount(server)
        .await;
}

pub fn labels() -> extras::Labels {
    extras::Labels {
        subtitles_qualifier: "Subtitles".to_owned(),
        metadata_qualifier: "Metadata".to_owned(),
        ai_track_title: "(AI Generated) {name}".to_owned(),
    }
}

pub fn empty_store() -> config::ConfigStore {
    config::ConfigStore::with_paths(
        std::path::PathBuf::from("/tmp/bilidown-test-nonexistent-config"),
        "/tmp",
    )
}

/// Episode fixture covering every field the NFO templates touch.
pub fn episode_task() -> database::TaskInfo {
    let mut task = database::TaskInfo::default();
    task.Basic.show_title = "T".to_owned();
    task.File.name = "N".to_owned();
    task.Episode.attribute = 1;
    task.Episode.bvid = "BV1".to_owned();
    task.Episode.cover = "C".to_owned();
    task.Episode.description = "P".to_owned();
    task.Episode.duration = 100;
    task.Episode.pubtime = 1_764_000_000;
    task.Episode.uploader = "U".to_owned();
    task.Episode.uploader_uid = 42;
    task.Episode.uploader_face = "F".to_owned();
    task.Episode.tags = vec!["a".to_owned(), "b".to_owned()];
    task
}

pub fn subtitle_body() -> Value {
    json!({
        "body": [
            {"from": 1.5, "to": 3.25, "content": "你好 world"},
            {"from": 10.0, "to": 12.5, "content": "第二行"}
        ]
    })
}

/// Pubtime in local time, formatted like `from_timestamp` would.
pub fn local_date(timestamp: i64) -> String {
    let naive = chrono::Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.naive_local())
        .unwrap();
    format!(
        "{:04}-{:02}-{:02}",
        naive.year(),
        naive.month(),
        naive.day()
    )
}
