//! b23.tv / bili2233.cn short-link expansion, and festival-page resolution.
//!
//! Short links carry no ids — the real link lives behind the redirect. This
//! module needs a `reqwest::Client` (from the future `network` crate) and
//! returns the expanded bilibili URL, ready for `classify`.
use std::sync::LazyLock;

use regex::Regex;
use reqwest::Client;

/// Why a short-link expansion failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    /// No `https://` in the link at all (Python: 无效的链接 / INVALID_LINK).
    InvalidLink,
    /// Redirect landed back on the short link itself — the link is expired
    /// (Python: B23_TV_URL_EXPIRED).
    Expired,
    /// HTTP or redirect failure while resolving.
    Request(String),
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::InvalidLink => write!(f, "invalid link: no https:// scheme found"),
            ExpandError::Expired => write!(f, "b23.tv url expired"),
            ExpandError::Request(e) => write!(f, "request failed: {e}"),
        }
    }
}
impl std::error::Error for ExpandError {}

/// Expand a b23.tv / bili2233.cn short link to its final bilibili URL.
///
/// Follows redirects (the Python client always follows). If the final URL is
/// the short link itself — the redirect target disappeared and bilibili
/// serves the short domain's own 200 page — the link is expired.
pub async fn expand_b23(client: &Client, url: &str) -> Result<String, ExpandError> {
    let normalized = normalize_url(url)?;
    let response = client
        .get(&normalized)
        .send()
        .await
        .map_err(|e| ExpandError::Request(e.to_string()))?;
    let final_url = response.url().to_string();

    if final_url == normalized {
        return Err(ExpandError::Expired);
    }
    Ok(final_url)
}

/// Resolve a festival page to its video URL: GET the page, scrape
/// `window.__INITIAL_STATE__` JSON for the video bvid.
///
/// Ports `FestivalParser.parse`: no-match, JSON-decode failure, and missing
/// `videoInfo.bvid` all surface as invalid-link errors (Python: 无效的链接).
pub async fn resolve_festival(client: &Client, url: &str) -> Result<String, ExpandError> {
    let normalized = normalize_url(url)?;
    let body = client
        .get(&normalized)
        .send()
        .await
        .map_err(|e| ExpandError::Request(e.to_string()))?
        .error_for_status()
        .map_err(|e| ExpandError::Request(e.to_string()))?
        .text()
        .await
        .map_err(|e| ExpandError::Request(e.to_string()))?;

    static INITIAL_STATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"window\.__INITIAL_STATE__\s*=\s*(\{.*?\})\s*;").unwrap());
    static BV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""bvid"\s*:\s*"([^"]+)""#).unwrap());

    let Some(caps) = INITIAL_STATE.captures(&body) else {
        return Err(ExpandError::InvalidLink);
    };
    // The non-greedy match may grab partial JSON; take the first bvid inside
    // the matched region instead of requiring full JSON decoding.
    let Some(bv) = BV.captures(&caps[1]).map(|c| c[1].to_owned()) else {
        return Err(ExpandError::InvalidLink);
    };
    Ok(format!("https://www.bilibili.com/video/{bv}"))
}

/// Strip garbage before the scheme; error when no scheme is present. Ports
/// `B23Parser._normalize_url` (which looks for `https://`; `http://` is
/// accepted too so local test servers work).
fn normalize_url(url: &str) -> Result<String, ExpandError> {
    if let Some(pos) = url.find("https://").or_else(|| url.find("http://")) {
        Ok(url[pos..].to_owned())
    } else {
        Err(ExpandError::InvalidLink)
    }
}
