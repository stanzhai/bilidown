//! Concurrent CDN host probing with health feedback (ticket 14).
//!
//! Ports the observable behavior of Python `util/network/download_url.py`:
//! a sliding window of [`PROBE_CONCURRENCY`] HEAD probes (Range GET
//! fallback) walks the whole candidate list, stops at the first hit, and
//! reports success/failure per **host** into the shared [`HostHealth`]
//! tracker (`cdn` module). No wall-clock budget: any time-based cutoff
//! would conflate "candidates remain untried" with "all nodes are bad",
//! and a retry would replay the same doomed timeline. Total time is
//! bounded structurally — one request per host (same-host candidates are
//! skipped once their host fails), at most two requests per candidate
//! (HEAD, then a Range GET when HEAD gives no size).
//!
//! [`resolve_download_url`] is the wire-facing entry the chunked
//! downloader consumes (ticket 13): it takes playurl links, yields the
//! chosen URL plus its file size. The client passed in should come from
//! [`HttpClient::probe`] — tight timeouts, zero retries, no cookies.

use std::collections::HashSet;

use crate::cdn::{netloc, report_host_failure, report_host_success, CdnConfig};
use crate::HttpClient;
use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::header::{HeaderMap, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::Method;

/// Probes in flight at once (`PROBE_CONCURRENCY`).
pub const PROBE_CONCURRENCY: usize = 4;

/// Hard per-probe timeout (`PROBE_TIMEOUT`): one bad node must not eat
/// the round; the next candidate is a cheaper bet than a retry.
pub const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Default minimum size for a usable download (`min_file_size` default).
pub const DEFAULT_MIN_FILE_SIZE: u64 = 1024;

/// A resolved download target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedUrl {
    pub url: String,
    pub file_size: u64,
}

/// Everything observable about why a resolve failed.
#[derive(Debug, thiserror::Error)]
#[error("无法获取有效的下载链接（{total} 个候选覆盖 {hosts} 个 CDN 节点，均已探测失败）")]
pub struct AllCandidatesFailed {
    /// Candidates in the flattened list.
    pub total: usize,
    /// Distinct hosts among them — probes are per-host, so failures are
    /// reported in host units ("12 candidates across 4 nodes"), not
    /// candidate units (which reads like "we only tried half").
    pub hosts: usize,
}

/// File size from response headers, 0 when unusable
/// (`_extract_file_size`): text/json bodies are error pages in disguise,
/// Content-Range's total wins over Content-Length (Range GETs).
fn extract_file_size(headers: &HeaderMap) -> u64 {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if content_type.is_empty() || content_type.contains("text") || content_type.contains("json") {
        return 0;
    }

    if let Some(range) = headers.get(CONTENT_RANGE).and_then(|v| v.to_str().ok()) {
        if let Some(total) = range
            .rsplit('/')
            .next()
            .map(str::trim)
            .and_then(|t| t.parse().ok())
        {
            return total;
        }
    }

    headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|l| l.trim().parse().ok())
        .unwrap_or(0)
}

/// Result of one candidate's probe.
struct ProbeOutcome {
    file_size: u64,
    /// Transport-level stall (our own pool queuing), not the node's
    /// fault: excluded from health reporting, retried once after the
    /// main sweep (`INCONCLUSIVE_REASONS`).
    inconclusive: bool,
}

enum ProbeAttempt {
    Size(u64),
    Inconclusive,
    Failed,
}

/// One candidate probe (`_probe_url`): HEAD; 405 or an unusable size
/// falls back to a `bytes=0-0` Range GET. No per-candidate retries —
/// candidates are interchangeable, the time belongs to the next untried
/// node; real transient faults are the task-level retry's problem.
async fn probe_once(client: &reqwest::Client, url: &str, min_file_size: u64) -> ProbeOutcome {
    let attempt = head_probe(client, url, min_file_size).await;
    match attempt {
        ProbeAttempt::Size(size) => ProbeOutcome {
            file_size: size,
            inconclusive: false,
        },
        ProbeAttempt::Inconclusive => ProbeOutcome {
            file_size: 0,
            inconclusive: true,
        },
        ProbeAttempt::Failed => ProbeOutcome {
            file_size: 0,
            inconclusive: false,
        },
    }
}

async fn head_probe(client: &reqwest::Client, url: &str, min_file_size: u64) -> ProbeAttempt {
    let response = match client.head(url).send().await {
        Ok(response) => response,
        Err(error) => return transport_class(error),
    };

    if response.status().as_u16() == 405 {
        return range_get_probe(client, url).await;
    }

    if !response.status().is_success() {
        return ProbeAttempt::Failed;
    }

    let size = extract_file_size(response.headers());
    if size > min_file_size {
        ProbeAttempt::Size(size)
    } else {
        range_get_probe(client, url).await
    }
}

async fn range_get_probe(client: &reqwest::Client, url: &str) -> ProbeAttempt {
    let response = match client
        .request(Method::GET, url)
        .header(RANGE, "bytes=0-0")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return transport_class(error),
    };

    if !response.status().is_success() {
        return ProbeAttempt::Failed;
    }

    ProbeAttempt::Size(extract_file_size(response.headers()))
}

/// Classify a reqwest transport error: request-pool timeouts are our own
/// queuing under load, not evidence about the node.
fn transport_class(error: reqwest::Error) -> ProbeAttempt {
    if error.is_request() && error.to_string().to_lowercase().contains("pool") {
        ProbeAttempt::Inconclusive
    } else {
        ProbeAttempt::Failed
    }
}

/// State shared across the sliding-window probes.
#[derive(Default)]
struct ProbeState {
    /// Hosts that conclusively failed this resolve: same-host candidates
    /// are skipped without a request — one node failure applies to every
    /// link on it.
    failed_hosts: HashSet<String>,

    inconclusive: Vec<String>,
}

/// Sliding-window sweep over the candidate list (`_probe_candidates`):
/// with every completion the next candidate is dispatched — the whole
/// batch is never awaited as one unit, or the slowest node would tax
/// every round. The sweep stops at the first hit by list position; after
/// flattening, list order *is* the user's mirror preference.
async fn probe_candidates(
    client: &reqwest::Client,
    candidates: &[String],
    min_file_size: u64,
    state: &parking_lot::Mutex<ProbeState>,
) -> Option<ResolvedUrl> {
    let mut index = 0;
    let mut pending: FuturesUnordered<tokio::task::JoinHandle<(usize, String, u64, bool)>> =
        FuturesUnordered::new();

    loop {
        while pending.len() < PROBE_CONCURRENCY && index < candidates.len() {
            let url = candidates[index].clone();
            let position = index;
            index += 1;

            if state.lock().failed_hosts.contains(&netloc(&url)) {
                continue;
            }

            let client = client.clone();
            pending.push(tokio::spawn(async move {
                let outcome = probe_once(&client, &url, min_file_size).await;
                (position, url, outcome.file_size, outcome.inconclusive)
            }));
        }

        let Some(joined) = pending.next().await else {
            break;
        };
        let Ok((_position, url, file_size, inconclusive)) = joined else {
            continue;
        };
        let host = netloc(&url);

        {
            let mut state = state.lock();

            if file_size > min_file_size {
                report_host_success(&host);
                return Some(ResolvedUrl { url, file_size });
            }
            if inconclusive {
                state.inconclusive.push(url);
                continue;
            }
            state.failed_hosts.insert(host.clone());
        }
        report_host_failure(&host);
    }

    None
}

/// Resolve a playurl link list to one downloadable URL
/// (`resolve_download_url`): build candidates from `config`, sweep every
/// host, return the first hit by list order. Inconclusive candidates get
/// exactly one make-up round after a fully failed sweep — pool queuing
/// under batch load is common and is not the node's fault.
pub async fn resolve_download_url(
    client: &HttpClient,
    url_list: &[String],
    config: &CdnConfig,
    min_file_size: u64,
) -> Result<ResolvedUrl, AllCandidatesFailed> {
    let candidates = config.candidate_urls(url_list);

    if candidates.is_empty() {
        return Err(AllCandidatesFailed { total: 0, hosts: 0 });
    }

    let state = parking_lot::Mutex::new(ProbeState::default());
    let inner = client.inner().clone();

    let mut result = probe_candidates(&inner, &candidates, min_file_size, &state).await;

    if result.is_none() {
        // Only the inconclusive deserve a make-up round; conclusive
        // failures must not be re-probed.
        let retry_list = std::mem::take(&mut state.lock().inconclusive);
        if !retry_list.is_empty() {
            result = probe_candidates(&inner, &retry_list, min_file_size, &state).await;
        }
    }

    match result {
        Some(resolved) => Ok(resolved),
        None => {
            let hosts = candidates
                .iter()
                .map(|u| netloc(u))
                .collect::<HashSet<_>>()
                .len();
            Err(AllCandidatesFailed {
                total: candidates.len(),
                hosts,
            })
        }
    }
}
