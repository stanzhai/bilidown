//! b23.tv / festival expansion against a wiremock server (the HTTP seam).
use urls::expand::{expand_b23, resolve_festival, ExpandError};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn expands_redirect_to_final_url() {
    let server = MockServer::start().await;
    let target = format!("{}/video/BV1xx411c7mD", server.uri());

    Mock::given(method("GET")).and(path_regex(r"^/abcdef$"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", &target))
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path_regex(r"^/video/.*$"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let short = format!("{}/abcdef", server.uri());
    assert_eq!(expand_b23(&client, &short).await, Ok(target));
}

#[tokio::test]
async fn strips_garbage_prefix() {
    // 链接前的无效字符被剥掉，以 https:// 开头为准
    let server = MockServer::start().await;
    let target = format!("{}/final", server.uri());
    Mock::given(method("GET")).and(path_regex(r"^/abcdef$"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", &target))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let dirty = format!("garbage {}/abcdef", server.uri());
    assert_eq!(expand_b23(&client, &dirty).await, Ok(target));
}

#[tokio::test]
async fn expired_short_link_lands_on_itself() {
    // 重定向回短链本身 = 链接失效（Python: B23_TV_URL_EXPIRED）
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path_regex(r"^/abcdef$"))
        .respond_with(ResponseTemplate::new(200).set_body_string("expired landing"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let short = format!("{}/abcdef", server.uri());
    assert_eq!(expand_b23(&client, &short).await, Err(ExpandError::Expired));
}

#[tokio::test]
async fn no_https_scheme_is_invalid() {
    let client = reqwest::Client::new();
    assert_eq!(expand_b23(&client, "b23.tv/abcdef").await, Err(ExpandError::InvalidLink));
}

#[tokio::test]
async fn festival_scrapes_initial_state_bvid() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path_regex(r"^/festival/.*$"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<html><script>window.__INITIAL_STATE__ = {"videoInfo":{"bvid":"BV1xx411c7mD"}};</script></html>"#,
        ))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let url = format!("{}/festival/abc", server.uri());
    assert_eq!(resolve_festival(&client, &url).await, Ok("https://www.bilibili.com/video/BV1xx411c7mD".to_owned()));
}

#[tokio::test]
async fn festival_without_initial_state_is_invalid() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path_regex(r"^/festival/.*$"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>nothing here</html>"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let url = format!("{}/festival/abc", server.uri());
    assert_eq!(resolve_festival(&client, &url).await, Err(ExpandError::InvalidLink));
}
