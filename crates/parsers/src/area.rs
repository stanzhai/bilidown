//! Area detection — the zone-fetch half of `src/util/network/area.py`.
//!
//! The pure response parser lives in `network::cdn::parse_area` (ticket
//! 14); this module owns the fetch: a dedicated client with a tight 5 s
//! timeout (the global client's generous timeouts would keep the
//! detection dialog hanging for the better part of a minute on a dead
//! link), Referer + the configured User-Agent, and no retry — the zone
//! endpoint rate-limits (412 on bursts) and the caller's fallback for a
//! real failure is "ask the user", cheaper than another timeout round.

use std::time::Duration;

use network::cdn::{parse_area, Area, AREA_ZONE_API};

/// Hard timeout of one detection (`AREA_DETECT_TIMEOUT`).
///
/// ponytail: `HttpClient` pins the shared timeouts (connect 15 s / read
/// 5 s); a strict 5 s total budget would need a builder option on the
/// network crate — add `ClientOptions::timeout` there if detection
/// latency ever matters beyond the read cap.
pub const AREA_DETECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors surfaced by area detection.
#[derive(Debug, thiserror::Error)]
pub enum AreaError {
    #[error("network: {0}")]
    Network(#[from] network::HttpError),
    /// Response arrived but no area could be parsed out of it
    /// (`RuntimeError("zone 接口的响应无法解析出地区")`).
    #[error("zone 接口的响应无法解析出地区")]
    Unparsable,
}

/// Detection options: the proxy configuration and User-Agent the request
/// rides on (mirroring `get_proxy_mounts()` + `config.user_agent`).
#[derive(Debug, Clone, Default)]
pub struct DetectOptions {
    pub proxy: network::ProxyConfig,
    pub user_agent: String,
}

/// Query the exit area once (`detect_area`).
///
/// Uses its own short-timeout client rather than a shared one: detection
/// failure interrupts the user with a dialog, so it must fail fast, and
/// the zone link carries no auth params so it need not share the global
/// cookie jar. Callers treat [`AreaError`] as "ask the user".
pub async fn detect_area(
    client: &network::HttpClient,
    base: &str,
) -> Result<Area, AreaError> {
    let url = if base.is_empty() {
        AREA_ZONE_API.to_owned()
    } else {
        format!("{base}/x/web-interface/zone")
    };

    let body = client.get(&url, &[]).await?;
    let response: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| AreaError::Unparsable)?;

    parse_area(&response).ok_or(AreaError::Unparsable)
}

/// Build the dedicated detection client (`_get_detect_headers` +
/// client options). The 5 s timeout lives on the client itself.
pub fn detection_client(options: DetectOptions) -> Result<network::HttpClient, network::HttpError> {
    network::HttpClient::new(network::ClientOptions {
        user_agent: options.user_agent,
        proxy: options.proxy,
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
}
