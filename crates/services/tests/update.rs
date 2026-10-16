//! Verhub update service tests (ticket 27), at the HTTP mock seam
//! (spec Testing Decisions #1): pinned wire shapes — paths, query strings,
//! headers, JSON bodies — and the domain results that must come out.
//!
//! Response bodies mirror the real Verhub API surface as consumed by
//! `verhub_sdk` 0.2.10 (`CheckUpdateResponse`, `VersionItem`,
//! `VersionListResponse`).

use network::{ClientOptions, HttpClient, ProxyConfig};
use services::update::{
    to_verhub_locale, CheckUpdateParams, UpdateError, VerhubClient, VERHUB_PROJECT_KEY,
};
use wiremock::matchers::{
    body_json, body_partial_json, header, header_regex, method, path, query_param,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = VERHUB_PROJECT_KEY;

fn check_body(include_preview: bool, locale: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "current_version": "2.21.0",
        "current_comparable_version": "2.20.0",
    });
    if include_preview {
        body["include_preview"] = serde_json::Value::Bool(true);
    }
    if let Some(l) = locale {
        body["locale"] = serde_json::Value::String(l.to_owned());
    }
    body
}

fn http() -> HttpClient {
    HttpClient::new(ClientOptions {
        user_agent: "test-ua".to_owned(),
        proxy: ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

fn latest(version: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "v-1", "version": version, "comparable_version": "2.22.0",
        "title": "标题", "content": "更新说明", "download_url": "https://dl/x.zip",
        "forced": false, "is_latest": true, "is_preview": false,
        "download_links": [], "platforms": [], "published_at": 0, "created_at": 0
    })
}

#[tokio::test]
async fn check_update_posts_current_versions_and_parses_verdict() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/public/{KEY}/versions/check-update")))
        .and(body_json(check_body(false, None)))
        .and(header("Accept", "application/json"))
        .and(header_regex(
            "User-Agent",
            r"^verhub-sdk-python/0\.2\.10 Bili23-Downloader/2\.21\.0$",
        ))
        .and(header("x-verhub-platform", "macos"))
        .and(header_regex(
            "x-verhub-client-time",
            r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}[+-]\d{2}:\d{2}$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "should_update": true, "required": false, "reason_codes": ["behind"],
            "current_version": "2.21.0", "current_comparable_version": "2.20.0",
            "latest_version": latest("2.22.0"),
            "latest_preview_version": null, "target_version": latest("2.22.0"),
            "milestone": {"current": false, "latest": false, "target_is_milestone": false}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let http = http();
    let client = VerhubClient::with_options(&http, &server.uri(), KEY, "2.21.0");
    let outcome = client
        .check_update(&CheckUpdateParams {
            current_version: "2.21.0".into(),
            current_comparable_version: "2.20.0".into(),
            include_preview: false,
            locale: None,
        })
        .await
        .unwrap();

    assert!(outcome.info.should_update);
    assert!(!outcome.info.required);
    assert_eq!(outcome.info.version, "2.22.0");
    assert_eq!(outcome.info.title.as_deref(), Some("标题"));
    assert_eq!(outcome.info.content, "更新说明");
    assert_eq!(outcome.info.update_url, "https://dl/x.zip");
    assert_eq!(outcome.reason_codes, vec!["behind".to_owned()]);
    server.verify().await;
}

#[tokio::test]
async fn check_update_sends_preview_and_locale_when_set() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/public/{KEY}/versions/check-update")))
        // SDK `compact` drops unset fields: preview/locale appear only when set
        .and(body_partial_json(check_body(true, Some("zh-CN"))))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "should_update": false, "required": false, "reason_codes": [],
            "latest_version": latest("2.20.0"), "target_version": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let http = http();
    let client = VerhubClient::with_options(&http, &server.uri(), KEY, "2.21.0");
    let outcome = client
        .check_update(&CheckUpdateParams {
            current_version: "2.21.0".into(),
            current_comparable_version: "2.20.0".into(),
            include_preview: true,
            locale: to_verhub_locale(Some("zh_CN")).map(str::to_owned),
        })
        .await
        .unwrap();

    assert!(!outcome.info.should_update);
    server.verify().await;
}

#[tokio::test]
async fn forced_update_is_reported_as_required() {
    let server = MockServer::start().await;
    let mut forced = latest("3.0.0");
    forced["forced"] = serde_json::Value::Bool(true);
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/public/{KEY}/versions/check-update")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "should_update": true, "required": true, "reason_codes": ["forced"],
            "latest_version": forced
        })))
        .expect(1)
        .mount(&server)
        .await;

    let http = http();
    let outcome = VerhubClient::with_options(&http, &server.uri(), KEY, "2.21.0")
        .check_update(&CheckUpdateParams::default())
        .await
        .unwrap();
    assert!(outcome.info.required && outcome.info.should_update);
}

#[tokio::test]
async fn error_response_surfaces_server_message_and_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/public/{KEY}/versions/check-update")))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "message": "current_version 未登记", "statusCode": 400
        })))
        .expect(1)
        .mount(&server)
        .await;

    let http = http();
    let err = VerhubClient::with_options(&http, &server.uri(), KEY, "2.21.0")
        .check_update(&CheckUpdateParams::default())
        .await
        .unwrap_err();
    match err {
        UpdateError::Api { status, message } => {
            assert_eq!(status, 400);
            assert_eq!(message, "current_version 未登记");
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    server.verify().await;
}

#[tokio::test]
async fn list_versions_passes_pagination_and_locale_as_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/public/{KEY}/versions")))
        .and(query_param("limit", "5"))
        .and(query_param("offset", "10"))
        .and(query_param("locale", "zh-TW"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total": 2,
            "data": [latest("2.22.0"), latest("2.21.0")]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let http = http();
    let list = VerhubClient::with_options(&http, &server.uri(), KEY, "2.21.0")
        .list_versions(5, 10, Some("zh-TW"))
        .await
        .unwrap();
    assert_eq!(list.total, 2);
    assert_eq!(list.data.len(), 2);
    assert_eq!(list.data[0].version, "2.22.0");
    server.verify().await;
}

#[tokio::test]
async fn latest_version_and_latest_preview_endpoints() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/public/{KEY}/versions/latest")))
        .and(query_param("locale", "zh-CN"))
        .respond_with(ResponseTemplate::new(200).set_body_json(latest("2.22.0")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/v1/public/{KEY}/versions/latest-preview"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Null))
        .expect(1)
        .mount(&server)
        .await;

    let http = http();
    let client = VerhubClient::with_options(&http, &server.uri(), KEY, "2.21.0");
    let latest = client.get_latest_version(Some("zh-CN")).await.unwrap();
    assert_eq!(latest.version, "2.22.0");
    // no preview published → null
    assert!(client
        .get_latest_preview_version(None)
        .await
        .unwrap()
        .is_none());
    server.verify().await;
}

/// Base URL trailing slash is normalized away (`_normalize_base_url`).
#[tokio::test]
async fn trailing_slash_in_base_url_is_normalized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/public/{KEY}/versions/latest")))
        .respond_with(ResponseTemplate::new(200).set_body_json(latest("2.22.0")))
        .expect(1)
        .mount(&server)
        .await;

    let http = http();
    let latest = VerhubClient::with_options(&http, &format!("{}/", server.uri()), KEY, "2.21.0")
        .get_latest_version(None)
        .await
        .unwrap();
    assert_eq!(latest.version, "2.22.0");
    server.verify().await;
}
