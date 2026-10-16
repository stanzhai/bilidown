//! External-behavior tests for the HTTP seam (spec Testing Decisions #1).
//!
//! Everything observable at the wire: headers, query params, cookies,
//! proxy routing, cookie persistence across client rebuilds, signing hook.

use network::{ClientOptions, HttpClient, ProxyConfig};
use wiremock::matchers::{header, method, path, query_param, header_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// `wiremock::matchers::header` splits values on commas (RFC list
/// semantics), so real User-Agent strings ("KHTML, like Gecko") never
/// exact-match; anchor a regex instead.
fn regex_escape(literal: &str) -> String {
    literal.replace('\\', "\\\\").replace('.', "\\.").replace('(', "\\(").replace(')', "\\)").replace('+', "\\+")
}

const TEST_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0";

fn options(ua: &str, proxy: ProxyConfig) -> ClientOptions {
    ClientOptions {
        user_agent: ua.to_owned(),
        proxy,
        cookies_from_persistence: Vec::new(),
        signer: None,
    }
}

#[tokio::test]
async fn sends_default_headers_and_configured_user_agent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/nav"))
        .and(header_regex("User-Agent", &format!("^{}$", regex_escape(TEST_UA))))
        .and(header("Referer", "https://www.bilibili.com/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": 0, "message": "0", "data": {"isLogin": false}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpClient::new(options(TEST_UA, ProxyConfig::none())).unwrap();
    let json: serde_json::Value = client
        .get_json(&format!("{}/x/web-interface/nav", server.uri()), &[])
        .await
        .unwrap();

    assert_eq!(json["data"]["isLogin"], false);
    server.verify().await;
}

#[tokio::test]
async fn request_params_reach_the_wire_as_query_string() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/v2"))
        .and(query_param("aid", "170001"))
        .and(query_param("cid", "280001"))
        .and(query_param("w_rid", "deadbeef"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": 0})))
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpClient::new(options(TEST_UA, ProxyConfig::none())).unwrap();
    let json: serde_json::Value = client
        .get_json(
            &format!("{}/x/player/wbi/v2", server.uri()),
            &[
                ("aid".to_owned(), "170001".to_owned()),
                ("cid".to_owned(), "280001".to_owned()),
                ("w_rid".to_owned(), "deadbeef".to_owned()),
            ],
        )
        .await
        .unwrap();

    assert_eq!(json["code"], 0);
    server.verify().await;
}

#[tokio::test]
async fn set_cookie_response_populates_jar_and_is_resent_on_next_request() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/first"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("set-cookie", "SESSDATA=abc123; Path=/; Domain=127.0.0.1")
                .set_body_json(serde_json::json!({"code": 0})),
        )
        .expect(1)
        .mount(&server)
        .await;

    // Second mock demands the cookie the first response installed.
    Mock::given(method("GET"))
        .and(path("/second"))
        .and(header("cookie", "SESSDATA=abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": 0})))
        .expect(1)
        .mount(&server)
        .await;

    let client = HttpClient::new(options(TEST_UA, ProxyConfig::none())).unwrap();
    let _: serde_json::Value = client
        .get_json(&format!("{}/first", server.uri()), &[])
        .await
        .unwrap();
    let second: serde_json::Value = client
        .get_json(&format!("{}/second", server.uri()), &[])
        .await
        .unwrap();

    assert_eq!(second["code"], 0);
    server.verify().await;
}

#[tokio::test]
async fn persisted_cookies_round_trip_across_client_rebuilds() {
    // Python behavior: login/anonymous cookies are persisted and replayed
    // after a restart. Observable: a rebuilt client seeded with the
    // snapshot sends those cookies on the wire.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/guarded"))
        .and(header_regex("cookie", "SESSDATA=sess-token-1"))
        .and(header_regex("cookie", "buvid3=bu3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    // Wiremock serves from 127.0.0.1; the cookie domain must match the
    // actual request host for the jar to replay it. In production the
    // domain is ".bilibili.com" (Python behavior); the mechanics tested
    // here are domain-scoping + replay, identical either way.
    let first = HttpClient::new(options(TEST_UA, ProxyConfig::none())).unwrap();
    first.set_cookie("SESSDATA", "sess-token-1", "127.0.0.1", "/");
    first.set_cookie("buvid3", "bu3", "127.0.0.1", "/");

    let snapshot = first.persisted_cookie_snapshot();
    assert_eq!(snapshot.len(), 2);

    let restarted = HttpClient::new(ClientOptions {
        user_agent: TEST_UA.to_owned(),
        proxy: ProxyConfig::none(),
        cookies_from_persistence: snapshot,
        signer: None,
    })
    .unwrap();

    let json: serde_json::Value = restarted
        .get_json(&format!("{}/guarded", server.uri()), &[])
        .await
        .unwrap();
    assert_eq!(json["ok"], true);
    server.verify().await;
}

#[tokio::test]
async fn signer_hook_appends_signature_params_before_the_request_leaves() {
    // The signing hook is the plug point the wbi ticket (08) uses: it sees
    // the request's params and returns extra ones (w_rid / wts).
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .and(query_param("aid", "1"))
        .and(query_param("w_rid", "hooked"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": 0})))
        .expect(1)
        .mount(&server)
        .await;

    struct HookedSigner;
    impl network::RequestSigner for HookedSigner {
        fn sign(&self, _url: &str, params: &[(String, String)]) -> Vec<(String, String)> {
            let _ = params;
            vec![("w_rid".to_owned(), "hooked".to_owned())]
        }
    }

    let mut opts = options(TEST_UA, ProxyConfig::none());
    opts.signer = Some(std::sync::Arc::new(HookedSigner));

    let client = HttpClient::new(opts).unwrap();
    let json: serde_json::Value = client
        .get_json(
            &format!("{}/x/player/wbi/playurl", server.uri()),
            &[("aid".to_owned(), "1".to_owned())],
        )
        .await
        .unwrap();

    assert_eq!(json["code"], 0);
    server.verify().await;
}

#[tokio::test]
async fn manual_proxy_routes_traffic_through_the_proxy_address() {
    // The proxy is a plain wiremock: a forward proxy receives the request
    // in absolute form (http://target/path) and answers it itself.
    let target = MockServer::start().await;

    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/web-interface/nav"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": 0})))
        .expect(1)
        .mount(&proxy)
        .await;

    let manual = ProxyConfig {
        mode: network::ProxyMode::Manual,
        proxy_type: network::ProxyType::Http,
        server: "127.0.0.1".to_owned(),
        port: proxy.address().port(),
        uname: String::new(),
        password: String::new(),
    };

    let client = HttpClient::new(options(TEST_UA, manual)).unwrap();
    let json: serde_json::Value = client
        .get_json(&format!("{}/x/web-interface/nav", target.uri()), &[])
        .await
        .unwrap();
    assert_eq!(json["code"], 0);

    // The request went through the proxy in absolute form; the target
    // server was never contacted directly.
    let received = proxy.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let sent_uri = received[0].url.as_str();
    assert!(
        sent_uri.starts_with(target.uri().as_str()),
        "proxy must receive the absolute-form target URI, got {sent_uri}"
    );
    assert_eq!(target.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn http_error_status_surfaces_as_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/boom"))
        .respond_with(ResponseTemplate::new(412))
        .mount(&server)
        .await;

    let client = HttpClient::new(options(TEST_UA, ProxyConfig::none())).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&format!("{}/boom", server.uri()), &[])
        .await
        .unwrap_err();
    assert!(err.is_status(), "412 must surface as a status error, got {err}");
}
