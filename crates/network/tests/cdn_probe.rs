//! Wiremock coverage for CDN probing (ticket 14): fake mirror servers
//! behind [`resolve_download_url`], asserting only external behavior —
//! which host got chosen, how many requests each mirror saw, and how
//! host health evolves.

use std::time::Duration;

use network::cdn::Area;
use network::cdn::{
    clear_host_health_for_test, host_is_cooling, report_host_failure, CdnConfig, CdnServer,
    PreferProvider, HOST_FAILURE_THRESHOLD,
};
use network::probe::{resolve_download_url, DEFAULT_MIN_FILE_SIZE};
use network::{ClientOptions, HttpClient};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn probe_client() -> HttpClient {
    HttpClient::probe(ClientOptions {
        user_agent: "bilidown-test/1.0".to_owned(),
        ..ClientOptions::default()
    })
    .expect("probe client")
}

fn config(servers: Vec<CdnServer>) -> CdnConfig {
    CdnConfig {
        area: Some(Area::Cn),
        cn_servers: servers,
        prefer_provider: PreferProvider(true),
        ..CdnConfig::default()
    }
}

/// Same contract as the unit-suite helper: hold the serial lock for the
/// whole sync test so a sibling's clear cannot wipe our cooldown mid-body.
fn clean() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    clear_host_health_for_test();
    guard
}

/// Async variant: a std MutexGuard cannot be held across an await point,
/// so the lock covers only the clear itself.
fn clean_async() {
    drop(clean());
}

#[tokio::test]
async fn probed_host_selection_ranks_by_candidate_order() {
    clean_async();

    let first = MockServer::start().await;
    let second = MockServer::start().await;
    let third = MockServer::start().await;

    // First candidate wins with a large file. With PROBE_CONCURRENCY=4
    // the sliding window legitimately opens later mirrors before the
    // winner answers, so a sibling may see one probe — never a hit.
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-length", "1048576"),
        )
        .expect(1)
        .mount(&first)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0..=1)
        .mount(&second)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0..=1)
        .mount(&third)
        .await;

    let servers = vec![
        CdnServer::new(&format!("127.0.0.1:{}", first.address().port()), "TEST"),
        CdnServer::new(&format!("127.0.0.1:{}", second.address().port()), "TEST"),
        CdnServer::new(&format!("127.0.0.1:{}", third.address().port()), "TEST"),
    ];
    let originals = vec!["http://upos-origin.example.net/video.m4s".to_owned()];

    let resolved = resolve_download_url(
        &probe_client(),
        &originals,
        &config(servers),
        DEFAULT_MIN_FILE_SIZE,
    )
    .await
    .expect("first mirror should hit");

    assert!(resolved.url.starts_with(first.uri().as_str()));
    assert_eq!(resolved.file_size, 1048576);
    first.verify().await;
    second.verify().await;
    third.verify().await;
}

#[tokio::test]
async fn first_mirror_down_second_answers() {
    clean_async();

    let bad = MockServer::start().await;
    let good = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&bad)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-length", "2097152"),
        )
        .expect(1)
        .mount(&good)
        .await;

    let servers = vec![
        CdnServer::new(&format!("127.0.0.1:{}", bad.address().port()), "TEST"),
        CdnServer::new(&format!("127.0.0.1:{}", good.address().port()), "TEST"),
    ];
    let originals = vec!["http://upos-origin.example.net/video.m4s".to_owned()];

    let resolved = resolve_download_url(
        &probe_client(),
        &originals,
        &config(servers),
        DEFAULT_MIN_FILE_SIZE,
    )
    .await
    .expect("second mirror should hit");

    assert!(resolved.url.starts_with(good.uri().as_str()));
    assert_eq!(resolved.file_size, 2097152);
    // The failed mirror's host entered cooldown after one conclusive
    // failure... wait, threshold is 2 — one failure is *not* cooling yet.
    assert!(!host_is_cooling(&format!(
        "127.0.0.1:{}",
        bad.address().port()
    )));
    bad.verify().await;
    good.verify().await;
}

#[test]
fn host_failures_accumulate_to_cooldown() {
    let _guard = clean();

    // Two consecutive failures on one host cross HOST_FAILURE_THRESHOLD
    // and demote it for the cooldown window.
    let host = "flaky.example.com";
    for _ in 0..HOST_FAILURE_THRESHOLD {
        report_host_failure(host);
    }
    assert!(host_is_cooling(host));
    clear_host_health_for_test();
    assert!(!host_is_cooling(host));
}

#[tokio::test]
async fn same_host_candidates_skipped_after_host_failure() {
    clean_async();

    let bad = MockServer::start().await;

    // The mirror host fails conclusively; the original link shares no
    // host with it, so exactly one HEAD reaches the mirror.
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&bad)
        .await;
    // A second path on the same host: never requested because the host
    // already failed conclusively in this resolve.
    Mock::given(method("HEAD"))
        .and(path("/other.m4s"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&bad)
        .await;

    let host = format!("127.0.0.1:{}", bad.address().port());
    let servers = vec![CdnServer::new(&host, "TEST")];
    // Two different original links both map onto the same mirror host:
    // after the first fails, the second is skipped without a request.
    let originals = vec![
        "http://upos-origin.example.net/video.m4s".to_owned(),
        "http://upos-backup.example.net/video.m4s".to_owned(),
    ];

    let error = resolve_download_url(
        &probe_client(),
        &originals,
        &config(servers),
        DEFAULT_MIN_FILE_SIZE,
    )
    .await
    .expect_err("all candidates should fail");

    // Candidates: both originals map onto the mirror but dedup to one
    // rewritten URL (same host, same path), plus the 2 originals.
    assert_eq!(error.total, 3);
    assert_eq!(error.hosts, 3);
    assert!(error.to_string().contains("无法获取有效的下载链接"));
    bad.verify().await;
}

#[tokio::test]
async fn head_405_falls_back_to_range_get() {
    clean_async();

    let server = MockServer::start().await;

    // HEAD is refused (405); the Range GET returns the true total via
    // Content-Range.
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(ResponseTemplate::new(405))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/video.m4s"))
        .and(wiremock::matchers::header("Range", "bytes=0-0"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-range", "bytes 0-0/3145728"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let host = format!("127.0.0.1:{}", server.address().port());
    let servers = vec![CdnServer::new(&host, "TEST")];
    let originals = vec!["http://upos-origin.example.net/video.m4s".to_owned()];

    let resolved = resolve_download_url(
        &probe_client(),
        &originals,
        &config(servers),
        DEFAULT_MIN_FILE_SIZE,
    )
    .await
    .expect("range fallback should hit");

    assert_eq!(resolved.file_size, 3145728);
    assert!(resolved.url.starts_with(server.uri().as_str()));
    server.verify().await;
}

#[tokio::test]
async fn text_error_page_is_failure() {
    clean_async();

    let server = MockServer::start().await;

    // 200 with text/html body: an error page in disguise. Content-Length
    // exists but the type makes the size unusable, so the probe fails
    // and the resolve falls through to the original link (also dead).
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .insert_header("content-length", "4096"),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/video.m4s"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .insert_header("content-length", "4096"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let host = format!("127.0.0.1:{}", server.address().port());
    let servers = vec![CdnServer::new(&host, "TEST")];
    let originals = vec!["http://upos-origin.example.net/video.m4s".to_owned()];

    let error = resolve_download_url(
        &probe_client(),
        &originals,
        &config(servers),
        DEFAULT_MIN_FILE_SIZE,
    )
    .await
    .expect_err("text/html must not count as a hit");

    // Mirror link + original = 2 candidates, 2 hosts.
    assert_eq!(error.total, 2);
    assert_eq!(error.hosts, 2);
    server.verify().await;
}

#[tokio::test]
async fn small_file_moves_on_to_next_candidate() {
    clean_async();

    let tiny = MockServer::start().await;
    let real = MockServer::start().await;

    // Tiny file: below DEFAULT_MIN_FILE_SIZE, so not a hit; the range
    // GET fallback confirms the small size and the probe moves on.
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-length", "100"),
        )
        .expect(1)
        .mount(&tiny)
        .await;
    Mock::given(method("GET"))
        .and(path("/video.m4s"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-range", "bytes 0-0/100"),
        )
        .expect(0..=1)
        .mount(&tiny)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/video.m4s"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-length", "8388608"),
        )
        .expect(1)
        .mount(&real)
        .await;

    let servers = vec![
        CdnServer::new(&format!("127.0.0.1:{}", tiny.address().port()), "TEST"),
        CdnServer::new(&format!("127.0.0.1:{}", real.address().port()), "TEST"),
    ];
    let originals = vec!["http://upos-origin.example.net/video.m4s".to_owned()];

    let resolved = resolve_download_url(
        &probe_client(),
        &originals,
        &config(servers),
        DEFAULT_MIN_FILE_SIZE,
    )
    .await
    .expect("second mirror should hit");

    assert!(resolved.url.starts_with(real.uri().as_str()));
    assert_eq!(resolved.file_size, 8388608);
    tiny.verify().await;
    real.verify().await;
}

#[tokio::test]
async fn probe_timeout_is_bounded() {
    clean_async();
    // The probe client must carry the documented per-probe timeout; a
    // dead node must not stall a resolve indefinitely.
    assert_eq!(network::probe::PROBE_TIMEOUT, Duration::from_secs(5));
}
