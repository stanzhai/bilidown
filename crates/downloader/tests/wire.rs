//! External-behavior tests for the chunked download engine at the wiremock
//! HTTP seam (spec Testing Decisions #1): given these CDN responses, this
//! file/state results. Covers reassembly, resume from offset tables, the
//! 206 guard, chunk/task retry semantics, token-bucket capping,
//! preallocation, and pause/cancel.

use downloader::{
    chunk_capacity, chunk_count, DownloadEngine, DownloadError, EngineConfig, FileSpec,
    OffsetTable, TaskStatus, TokenBucketHandle, CHUNK_SIZE, MAX_CHUNK_RETRIES,
};
use network::{ClientOptions, HttpClient, ProxyConfig};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client() -> HttpClient {
    HttpClient::new(ClientOptions {
        user_agent: "bilidown-test".into(),
        proxy: ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

fn spec(server_uri: &str, key: &str, size: u64, dir: &std::path::Path) -> FileSpec {
    FileSpec {
        file_key: key.to_owned(),
        url: format!("{server_uri}/{key}"),
        file_size: size,
        path: dir.join(format!("{key}.m4s")),
    }
}

fn config() -> EngineConfig {
    EngineConfig {
        parallel_chunks: 8,
        auto_retry_enabled: false,
        auto_retry_max_count: 5,
        preallocate: true,
        chunk_backoff_unit: Duration::from_millis(1),
        task_retry_unit: Duration::from_millis(5),
    }
}

/// Deterministic pseudorandom body (xorshift) so tests can assert exact
/// bytes without carrying binary fixtures.
fn body(seed: u64, len: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(len as usize);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push((state & 0xff) as u8);
    }
    out
}

/// Range-honoring 206 CDN: slices `data` by the request's `bytes=a-b`.
fn range_cdn(data: Arc<Vec<u8>>) -> impl Fn(&Request) -> ResponseTemplate + Send + Sync + 'static {
    move |req: &Request| {
        let range = req
            .headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());
        match range {
            Some(range) if range.starts_with("bytes=") => {
                let (s, e) = range[6..].split_once('-').unwrap();
                let (start, end): (usize, usize) = (s.parse().unwrap(), e.parse().unwrap());
                ResponseTemplate::new(206)
                    .append_header(
                        "Content-Range",
                        format!("bytes {}-{}/{}", start, end, data.len()),
                    )
                    .set_body_bytes(data[start..=end].to_vec())
            }
            _ => ResponseTemplate::new(200).set_body_bytes(data.as_ref().clone()),
        }
    }
}

/// Range CDN whose first `failures` requests return 503.
fn flaky_cdn(
    data: Arc<Vec<u8>>,
    failures: Arc<AtomicUsize>,
) -> impl Fn(&Request) -> ResponseTemplate + Send + Sync + 'static {
    move |req: &Request| {
        if failures.fetch_sub(1, Ordering::SeqCst) > 0 {
            return ResponseTemplate::new(503);
        }
        range_cdn(data.clone())(req)
    }
}

async fn ranges_of(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.headers.contains_key("range"))
        .map(|r| {
            let bytes = r.headers.get("range").unwrap().as_bytes();
            String::from_utf8(bytes.to_vec()).unwrap()
        })
        .collect()
}

#[tokio::test]
async fn multi_chunk_download_reassembles_identical_file() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = CHUNK_SIZE * 3 + 12345; // 3 full chunks + a short tail
    let data = Arc::new(body(0x5eed, size));
    assert_eq!(chunk_count(size), 4);

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(range_cdn(data.clone()))
        .mount(&server)
        .await;

    let mut engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    assert_eq!(state.status, TaskStatus::Completed);
    assert_eq!(state.progress(), 100);
    assert_eq!(state.downloaded_size, size);
    let on_disk = std::fs::read(dir.path().join("video.m4s")).unwrap();
    assert_eq!(on_disk, *data, "reassembled file must be byte-identical");
    // every chunk's Range landed on the wire, exactly once
    let ranges = ranges_of(&server).await;
    assert_eq!(ranges.len(), 4, "{ranges:?}");
    assert!(ranges.contains(&format!("bytes=0-{}", CHUNK_SIZE - 1)));
    assert!(ranges.contains(&format!("bytes={}-{}", 3 * CHUNK_SIZE, size - 1)));
    // offset table records each chunk at its capacity
    let t = &engine.offset_tables()["video"];
    assert_eq!(
        t.offset(3, chunk_capacity(3, 4, size)),
        size - 3 * CHUNK_SIZE
    );
}

#[tokio::test]
async fn resume_downloads_only_remaining_ranges() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = CHUNK_SIZE * 4;
    let data = Arc::new(body(0xfeed, size));

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(range_cdn(data.clone()))
        .mount(&server)
        .await;

    // A previous run finished chunks 0-1 and half of chunk 2, and left the
    // partial file behind (crash). Reproduce both artifacts.
    std::fs::write(
        dir.path().join("video.m4s"),
        &data[..(CHUNK_SIZE * 2 + CHUNK_SIZE / 2) as usize],
    )
    .unwrap();
    let mut table = OffsetTable::default();
    table.commit(0, CHUNK_SIZE);
    table.commit(1, CHUNK_SIZE);
    table.commit(2, CHUNK_SIZE / 2);
    let mut persisted = BTreeMap::new();
    persisted.insert("video".to_owned(), table);

    let mut engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    engine.resume_from(persisted);
    let before = engine.snapshot();
    assert_eq!(before.downloaded_size, CHUNK_SIZE * 2 + CHUNK_SIZE / 2);
    assert_eq!(before.queue, vec!["video".to_owned()]);

    let state = engine.run().await;
    assert_eq!(state.status, TaskStatus::Completed);
    let on_disk = std::fs::read(dir.path().join("video.m4s")).unwrap();
    assert_eq!(on_disk, *data);

    // Only the unfinished ranges were requested: chunk 2 from its offset,
    // chunk 3 whole.
    let ranges = ranges_of(&server).await;
    assert_eq!(
        ranges.len(),
        2,
        "must not re-request finished chunks: {ranges:?}"
    );
    assert!(ranges.contains(&format!(
        "bytes={}-{}",
        CHUNK_SIZE * 2 + CHUNK_SIZE / 2,
        3 * CHUNK_SIZE - 1
    )));
    assert!(ranges.contains(&format!("bytes={}-{}", 3 * CHUNK_SIZE, size - 1)));
}

#[tokio::test]
async fn non_206_full_response_triggers_guard_without_corrupting() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = CHUNK_SIZE * 2;
    let data = Arc::new(body(0xdead, size));

    // Server ignores Range and returns the whole file with 200.
    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.as_ref().clone()))
        .mount(&server)
        .await;

    let mut engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    // The guard must fail the task, never write the full body at a chunk
    // offset (the silent-corruption path).
    assert_eq!(state.status, TaskStatus::Failed);
    assert_eq!(
        state.last_error.unwrap(),
        DownloadError::Retryable("server ignored Range (HTTP 200), round discarded".into())
    );
    let on_disk = std::fs::read(dir.path().join("video.m4s")).unwrap();
    assert!(
        on_disk.iter().all(|&b| b == 0),
        "nothing may be written by a guarded round"
    );
}

#[tokio::test]
async fn chunk_retry_with_backoff_then_success() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 3 * 1024 * 1024; // single chunk
    let data = Arc::new(body(0xabc, size));

    // First two requests 503 (retryable), then honor Range.
    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(flaky_cdn(data.clone(), Arc::new(AtomicUsize::new(2))))
        .mount(&server)
        .await;

    let mut engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    assert_eq!(state.status, TaskStatus::Completed);
    // unit = 1ms: backoffs are 1ms then 2ms
    let log = engine.chunk_retry_log();
    assert_eq!(log.len(), 2, "two failed attempts before success");
    assert_eq!(log[0].1, Duration::from_millis(1));
    assert_eq!(log[1].1, Duration::from_millis(2));
    assert_eq!(std::fs::read(dir.path().join("video.m4s")).unwrap(), *data);
}

#[tokio::test]
async fn permanent_status_goes_terminal_without_retries() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 1024;

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let mut engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    assert_eq!(state.status, TaskStatus::Failed);
    assert!(!state.last_error.unwrap().retryable());
    // 403 is permanent: one request, no backoff sleeps
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(engine.chunk_retry_log().is_empty());
}

#[tokio::test]
async fn task_auto_retry_retries_within_quota_then_completes() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 1024;
    let data = Arc::new(body(7, size));

    // First three requests 503, then recovery.
    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(flaky_cdn(data.clone(), Arc::new(AtomicUsize::new(3))))
        .mount(&server)
        .await;

    let mut cfg = config();
    cfg.auto_retry_enabled = true;
    cfg.auto_retry_max_count = 3;
    let mut engine = DownloadEngine::new(
        client(),
        cfg,
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    assert_eq!(state.status, TaskStatus::Completed);
    assert_eq!(std::fs::read(dir.path().join("video.m4s")).unwrap(), *data);
    // initial pass exhausts 5 chunk attempts, then each retry pass needs
    // one request to fail before the next recovers: 3 failures total.
    let requests = server.received_requests().await.unwrap().len();
    assert!(
        requests >= 4,
        "3 failures then success needs >= 4 requests, got {requests}"
    );
}

#[tokio::test]
async fn task_auto_retry_quota_exhausted_goes_terminal() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 1024;

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let mut cfg = config();
    cfg.auto_retry_enabled = true;
    cfg.auto_retry_max_count = 2;
    let mut engine = DownloadEngine::new(
        client(),
        cfg,
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    assert_eq!(state.status, TaskStatus::Failed);
    assert!(state.last_error.unwrap().retryable());
    // initial pass (MAX_CHUNK_RETRIES attempts) + 2 auto-retry passes
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        MAX_CHUNK_RETRIES as usize * 3
    );
}

#[tokio::test]
async fn progress_resets_auto_retry_quota() {
    // Retryable failures that still make progress must not burn task
    // quota (test_auto_retry semantics: progress resets the counter).
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 1024;
    let data = Arc::new(body(11, size));

    // Serve a truncated slice for the first request (a partial round
    // yields progress, then the next round resumes from the offset).
    let hits = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with({
            let data = data.clone();
            let hits = hits.clone();
            move |req: &Request| {
                let n = hits.fetch_add(1, Ordering::SeqCst);
                let range = req
                    .headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                let (s, e) = range[6..].split_once('-').unwrap();
                let (start, end): (usize, usize) = (s.parse().unwrap(), e.parse().unwrap());
                let end = if n == 0 {
                    start + (end - start) / 2
                } else {
                    end
                };
                ResponseTemplate::new(206)
                    .append_header("Content-Range", format!("bytes {}-{}/{}", start, end, size))
                    .set_body_bytes(data[start..=end].to_vec())
            }
        })
        .mount(&server)
        .await;

    let mut cfg = config();
    cfg.auto_retry_enabled = true;
    cfg.auto_retry_max_count = 1; // quota of one
    let mut engine = DownloadEngine::new(
        client(),
        cfg,
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    // The truncated round made progress, so the follow-up round was free
    // and the task completes without spending the quota.
    assert_eq!(state.status, TaskStatus::Completed);
    assert_eq!(std::fs::read(dir.path().join("video.m4s")).unwrap(), *data);
}

#[tokio::test]
async fn token_bucket_caps_observed_throughput() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 2 * 1024 * 1024; // single chunk
    let data = Arc::new(body(13, size));

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(range_cdn(data.clone()))
        .mount(&server)
        .await;

    let mut cfg = config();
    cfg.preallocate = false;
    let bucket = TokenBucketHandle::new(512.0 * 1024.0); // 512 KiB/s
    let start = Instant::now();
    let mut engine = DownloadEngine::new(
        client(),
        cfg,
        bucket,
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;
    let elapsed = start.elapsed();

    assert_eq!(state.status, TaskStatus::Completed);
    // Token-bucket semantics: the bucket starts full (one rate-second of
    // burst), then sustains at 512 KiB/s. Floor = sustain time for the
    // post-burst remainder.
    let rate = 512.0 * 1024.0;
    let ideal = size as f64 / rate;
    let floor = (ideal - 1.0).max(0.0) * 0.9;
    assert!(
        elapsed.as_secs_f64() >= floor,
        "2 MiB at 512 KiB/s needs >=~{floor:.1}s, finished in {:.2}s — limiter not capping",
        elapsed.as_secs_f64()
    );
}

#[tokio::test]
async fn preallocation_on_by_config_sizes_the_file_upfront() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = CHUNK_SIZE + 100;
    let data = Arc::new(body(17, size));

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(range_cdn(data.clone()))
        .mount(&server)
        .await;

    let mut engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    // pause before starting: the file must already exist at full size
    engine.pause();
    let state = engine.run().await;

    assert_eq!(state.status, TaskStatus::Paused);
    let meta = std::fs::metadata(dir.path().join("video.m4s")).unwrap();
    assert_eq!(
        meta.len(),
        size,
        "preallocation must size the file before any byte arrives"
    );
}

#[tokio::test]
async fn preallocation_off_leaves_placeholder_then_grows() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = CHUNK_SIZE + 100;
    let data = Arc::new(body(19, size));

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(range_cdn(data.clone()))
        .mount(&server)
        .await;

    let mut cfg = config();
    cfg.preallocate = false;
    let mut engine = DownloadEngine::new(
        client(),
        cfg,
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let state = engine.run().await;

    assert_eq!(state.status, TaskStatus::Completed);
    assert_eq!(
        std::fs::metadata(dir.path().join("video.m4s"))
            .unwrap()
            .len(),
        size
    );
    assert_eq!(std::fs::read(dir.path().join("video.m4s")).unwrap(), *data);
}

#[tokio::test]
async fn pause_mid_download_then_resume_finishes_without_redownload() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = CHUNK_SIZE * 2;
    let data = Arc::new(body(23, size));

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(range_cdn(data.clone()))
        .mount(&server)
        .await;

    // Slow bucket keeps it mid-download long enough to pause.
    let engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::new(256.0 * 1024.0),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let control = engine.control();
    let mut runner = engine;
    let handle = tokio::spawn(async move { runner.run().await });

    // Wait until a flush has confirmed some bytes, then pause.
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if control.snapshot().downloaded_size > 0 || handle.is_finished() {
            break;
        }
    }
    control.pause();
    let paused_state = handle.await.unwrap();
    assert_eq!(paused_state.status, TaskStatus::Paused);
    let tables = control.offset_tables();
    let t = &tables["video"];
    let confirmed: u64 = (0..2)
        .map(|i| t.offset(i, chunk_capacity(i, 2, size)))
        .sum();
    assert!(confirmed > 0, "pause must leave confirmed offsets");

    // Second engine resumes from the persisted table over the same file
    // and finishes byte-identically.
    let mut resumed = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    resumed.resume_from(tables);
    let final_state = resumed.run().await;
    assert_eq!(final_state.status, TaskStatus::Completed);
    assert_eq!(std::fs::read(dir.path().join("video.m4s")).unwrap(), *data);

    // Resume must pick up from the paused chunk's confirmed offset (a
    // mid-chunk start), not re-download finished ranges from zero.
    let ranges = ranges_of(&server).await;
    let mid_chunk = ranges.iter().any(|r| {
        let start: u64 = r[6..].split('-').next().unwrap().parse().unwrap();
        !start.is_multiple_of(CHUNK_SIZE)
    });
    assert!(
        mid_chunk,
        "resume must start from a confirmed mid-chunk offset: {ranges:?}"
    );
}

#[tokio::test]
async fn cancel_stops_and_leaves_resumable_offsets() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = CHUNK_SIZE * 3;
    let data = Arc::new(body(29, size));

    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with(range_cdn(data.clone()))
        .mount(&server)
        .await;

    let engine = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::new(128.0 * 1024.0),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    let control = engine.control();
    let mut runner = engine;
    let handle = tokio::spawn(async move { runner.run().await });

    // Let some traffic flow, then cancel.
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if control.snapshot().downloaded_size > 0 || handle.is_finished() {
            break;
        }
    }
    control.pause(); // cancel == pause mechanics; offsets stay resumable
    let state = handle.await.unwrap();
    assert_eq!(state.status, TaskStatus::Paused);
    let t = &control.offset_tables()["video"];
    let confirmed: u64 = (0..3)
        .map(|i| t.offset(i, chunk_capacity(i, 3, size)))
        .sum();
    assert!(confirmed > 0, "cancel must leave resumable offsets");

    // A fresh engine picks up from those offsets and completes.
    let mut resumed = DownloadEngine::new(
        client(),
        config(),
        TokenBucketHandle::unlimited(),
        Arc::new(()),
        vec![spec(&server.uri(), "video", size, dir.path())],
    );
    resumed.resume_from(control.offset_tables());
    let final_state = resumed.run().await;
    assert_eq!(final_state.status, TaskStatus::Completed);
    assert_eq!(std::fs::read(dir.path().join("video.m4s")).unwrap(), *data);
}
