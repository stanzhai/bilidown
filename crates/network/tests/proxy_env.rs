//! Proxy-mode env interaction. Separate test binary: mutating http_proxy
//! env vars would race sibling tests in the same process.

use network::{ClientOptions, HttpClient, ProxyConfig, ProxyMode};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_UA: &str = "test-agent";

#[tokio::test]
async fn direct_mode_ignores_environment_proxies() {
    // ProxyMode::None must connect straight to the server even when
    // proxy env vars point at a black hole. (Python: mounts=None → direct.)
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/nav"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": 0})))
        .expect(1)
        .mount(&server)
        .await;

    for key in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        std::env::set_var(key, "http://127.0.0.1:1");
    }

    let client = HttpClient::new(ClientOptions {
        user_agent: TEST_UA.to_owned(),
        proxy: ProxyConfig { mode: ProxyMode::None, ..ProxyConfig::none() },
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap();

    let json: serde_json::Value = client
        .get_json(&format!("{}/nav", server.uri()), &[])
        .await
        .unwrap();
    assert_eq!(json["code"], 0);
    server.verify().await;
}

#[tokio::test]
async fn system_mode_uses_environment_proxy() {
    // ProxyMode::System restores the httpx-default behavior the Python
    // client had: env/system proxies are honored.
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/nav"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": 0})))
        .expect(1)
        .mount(&proxy)
        .await;

    // All six variants: test 1 set them to a black hole, and reqwest may
    // prefer the uppercase/all_proxy form when re-reading the environment.
    for key in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        std::env::set_var(key, proxy.uri());
    }

    let client = HttpClient::new(ClientOptions {
        user_agent: TEST_UA.to_owned(),
        proxy: ProxyConfig { mode: ProxyMode::System, ..ProxyConfig::none() },
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap();

    let target = "http://example.invalid/nav";
    let json: serde_json::Value = client.get_json(target, &[]).await.unwrap();
    assert_eq!(json["code"], 0);
    assert_eq!(proxy.received_requests().await.unwrap().len(), 1);
}
