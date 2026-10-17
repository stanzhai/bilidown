//! Auto-parse pagination walk (`auto_parse.rs::walk_pages`) at the HTTP
//! seam: wiremock serves three space pages, the walk must fetch them in
//! order, fire progress after each, honor the stop flag between pages,
//! and collect every page's episodes into one outcome.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parsers::auto_parse::{page_progress_text, walk_pages, AutoParseOptions};
use parsers::list::{parse_space, ListError, ListParseContext};
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn space_page(count: i64) -> Value {
    json!({
        "code": 0,
        "data": {
            "list": {"vlist": [
                {"aid": 1, "bvid": "BVwalk1", "pic": "p", "title": "投稿", "created": 1700000000, "length": "1:00"},
            ]},
            "page": {"pn": 1, "ps": 40, "count": count},
        }
    })
}

async fn mount_page(server: &MockServer, pn: u16, count: i64) {
    Mock::given(method("GET"))
        .and(path("/x/space/wbi/arc/search"))
        .and(query_param("pn", pn.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(space_page(count)))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_card(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/x/web-interface/card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "data": {"card": {"name": "UP"}}
        })))
        .mount(server)
        .await;
}

fn client() -> network::HttpClient {
    network::HttpClient::new(network::ClientOptions {
        user_agent: String::new(),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

#[tokio::test]
async fn walk_fetches_every_page_and_reports_progress() {
    let server = MockServer::start().await;
    mount_page(&server, 1, 120).await;
    mount_card(&server).await;
    mount_page(&server, 2, 120).await;
    mount_page(&server, 3, 120).await;
    let client = Arc::new(client());
    let ctx = Arc::new(ListParseContext::new());
    let url = Arc::new("https://space.bilibili.com/123/video".to_owned());
    let base = server.uri();

    let texts = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = texts.clone();

    let outcome = walk_pages(
        &AutoParseOptions {
            start_page: 1,
            end_page: 3,
            interval: Duration::ZERO,
        },
        move |pn| {
            let (client, ctx, url, base) = (client.clone(), ctx.clone(), url.clone(), base.clone());
            Box::pin(async move { parse_space(&client, &base, &ctx, &url, pn).await })
        },
        |text, _page, _total| sink.lock().push(text.to_owned()),
        || false,
    )
    .await
    .unwrap();

    assert_eq!(outcome.pages.len(), 3);
    assert_eq!(outcome.episodes.len(), 3, "每一页的条目都要并入同一结果");
    assert_eq!(
        *texts.lock(),
        vec![
            page_progress_text(1, 3),
            page_progress_text(2, 3),
            page_progress_text(3, 3),
        ]
    );
    assert_eq!(
        texts.lock()[0],
        "Parsing page 1, total 3 pages, progress: 33%"
    );
}

#[tokio::test]
async fn stop_flag_ends_the_walk_between_pages() {
    let server = MockServer::start().await;
    mount_page(&server, 1, 120).await;
    mount_card(&server).await;
    // Pages 2 and 3 stay unmounted: any request there fails the test via
    // the unexpected-request panic, proving the walk stopped in time.
    let client = Arc::new(client());
    let ctx = Arc::new(ListParseContext::new());
    let url = Arc::new("https://space.bilibili.com/123/video".to_owned());
    let base = server.uri();

    let calls = Arc::new(AtomicUsize::new(0));
    let stop = calls.clone();
    let outcome = walk_pages(
        &AutoParseOptions {
            start_page: 1,
            end_page: 3,
            interval: Duration::ZERO,
        },
        move |pn| {
            let (client, ctx, url, base) = (client.clone(), ctx.clone(), url.clone(), base.clone());
            Box::pin(async move { parse_space(&client, &base, &ctx, &url, pn).await })
        },
        |_, _, _| {},
        move || stop.fetch_add(1, Ordering::SeqCst) >= 1,
    )
    .await
    .unwrap();

    assert_eq!(outcome.pages.len(), 1, "停止标志生效后不再取后续页");
    assert_eq!(outcome.episodes.len(), 1);
}

#[tokio::test]
async fn page_error_stops_the_walk() {
    // Error propagation: a page failure must surface, not be swallowed.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x/space/wbi/arc/search"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"code": -404, "message": "啥都木有"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = Arc::new(client());
    let ctx = Arc::new(ListParseContext::new());
    let url = Arc::new("https://space.bilibili.com/123/video".to_owned());
    let base = server.uri();

    let error = walk_pages(
        &AutoParseOptions {
            start_page: 1,
            end_page: 2,
            interval: Duration::ZERO,
        },
        move |pn| {
            let (client, ctx, url, base) = (client.clone(), ctx.clone(), url.clone(), base.clone());
            Box::pin(async move { parse_space(&client, &base, &ctx, &url, pn).await })
        },
        |_, _, _| {},
        || false,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ListError::Api(_)));
}
