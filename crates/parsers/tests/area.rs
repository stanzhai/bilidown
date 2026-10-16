//! External-behavior tests for area detection: the zone fetch against
//! wiremock with the real response bodies from
//! `test/test_area_detection.py`, plus the failure paths.

use parsers::area::{detect_area, AreaError};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use network::cdn::Area;
use network::{ClientOptions, HttpClient, ProxyConfig};

fn client() -> HttpClient {
    HttpClient::new(ClientOptions {
        user_agent: "bilidown-rs/test".to_owned(),
        proxy: ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

/// test_area_detection.py:48-61 — a real mainland zone response.
const MAINLAND_BODY: &str = r#"{
    "code": 0, "message": "OK", "ttl": 1,
    "data": {
        "addr": "221.7.112.22", "country": "中国", "province": "重庆",
        "isp": "联通", "country_code": 86
    }
}"#;

/// test_area_detection.py:63-80 — a real Hong Kong zone response.
const HONGKONG_BODY: &str = r#"{
    "code": 0, "message": "OK", "ttl": 1,
    "data": {
        "addr": "157.254.20.4", "country": "香港",
        "latitude": 22.396428, "longitude": 114.109497,
        "zone_id": 1035993088, "country_code": 852
    }
}"#;

#[tokio::test]
async fn mainland_response_detects_cn() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/zone"))
        .and(header("referer", "https://www.bilibili.com/"))
        .and(header("user-agent", "bilidown-rs/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(MAINLAND_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let area = detect_area(&client(), &server.uri()).await.unwrap();

    assert_eq!(area, Area::Cn);
    server.verify().await;
}

#[tokio::test]
async fn hongkong_response_detects_cn() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/zone"))
        .respond_with(ResponseTemplate::new(200).set_body_string(HONGKONG_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let area = detect_area(&client(), &server.uri()).await.unwrap();

    // country_code 852 is in the China set (CHINA_COUNTRY_CODES).
    assert_eq!(area, Area::Cn);
    server.verify().await;
}

#[tokio::test]
async fn overseas_response_detects_ov() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/zone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": 0, "message": "OK", "ttl": 1,
            "data": {
                "addr": "203.0.113.9", "country": "日本",
                "country_code": 81
            }
        })))
        .mount(&server)
        .await;

    let area = detect_area(&client(), &server.uri()).await.unwrap();

    assert_eq!(area, Area::Ov);
}

#[tokio::test]
async fn http_failure_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/zone"))
        .respond_with(ResponseTemplate::new(412))
        .mount(&server)
        .await;

    let error = detect_area(&client(), &server.uri()).await.unwrap_err();

    assert!(matches!(error, AreaError::Network(_)));
}

#[tokio::test]
async fn unparsable_body_is_the_runtime_error_port() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/zone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": -352, "message": "风控校验失败",
        })))
        .mount(&server)
        .await;

    let error = detect_area(&client(), &server.uri()).await.unwrap_err();

    match error {
        AreaError::Unparsable => {}
        other => panic!("expected Unparsable, got {other:?}"),
    }
}

#[tokio::test]
async fn garbage_body_is_unparsable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/zone"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let error = detect_area(&client(), &server.uri()).await.unwrap_err();

    assert!(matches!(error, AreaError::Unparsable));
}
