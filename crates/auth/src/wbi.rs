//! wbi request signing — port of `util/parse/parser/base.py`'s `enc_wbi`.
//!
//! The two keys are session-scoped (never persisted by the Python build:
//! `store_wbi_keys` writes with `save=False` so a stale key can't sign its
//! way into a `-403` next launch). Every nav response that carries
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use md5::Md5;
use parking_lot::RwLock;

use digest::Digest;
use network::RequestSigner;

/// `mixinKeyEncTab` — index permutation over `img_key + sub_key`.
pub const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// `max(mixinKeyEncTab) + 1` — the concatenated key length `getMixinKey`
pub const WBI_KEY_LENGTH: usize = 64;

/// Characters stripped from every param value before signing (Python
/// `filter(lambda chr: chr not in "!'()*", str(v))`).
const STRIPPED: [char; 5] = ['!', '\'', '(', ')', '*'];

/// `https://api.bilibili.com/x/web-interface/nav` (Python `NAV_API`).
pub const NAV_API: &str = "https://api.bilibili.com/x/web-interface/nav";

/// `store_wbi_keys(data)` — extract the keys from a nav response's
/// `data.wbi_img`, taking `Path(url).stem` of each URL. Empty strings
/// (`None` here) mean "no keys in this response".
pub fn wbi_keys_from_nav(data: &serde_json::Value) -> Option<(String, String)> {
    let img_url = data.get("wbi_img")?.get("img_url")?.as_str()?;
    let sub_url = data.get("wbi_img")?.get("sub_url")?.as_str()?;

    if img_url.is_empty() || sub_url.is_empty() {
        return None;
    }

    Some((
        file_stem(img_url)?.to_owned(),
        file_stem(sub_url)?.to_owned(),
    ))
}

/// `Path(url).stem` — the filename without directories or extension.
fn file_stem(url: &str) -> Option<&str> {
    let name = url.rsplit(['/', '\\']).next()?;
    match name.rfind('.') {
        Some(0) | None => Some(name),
        Some(dot) => name.get(..dot),
    }
}

/// Derive the 32-char mixin key (Python `getMixinKey`), or `Err` when the
/// concatenated keys are shorter than the table's largest index. The error
/// text must contain "signature key" (test_wbi_signature pins that).
pub fn mixin_key(img_key: &str, sub_key: &str) -> Result<String, WbiError> {
    let orig = format!("{img_key}{sub_key}");
    if orig.chars().count() < WBI_KEY_LENGTH {
        return Err(WbiError::KeysUnavailable);
    }
    let mut key = String::with_capacity(32);
    for &i in MIXIN_KEY_ENC_TAB.iter().take(32) {
        key.push(orig.as_bytes()[i] as char);
    }
    Ok(key)
}

/// One signed query: full param list (sorted, `wts` included, values
/// filtered) plus the `w_rid` digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedParams {
    pub params: Vec<(String, String)>,
    pub w_rid: String,
}

impl SignedParams {
    /// Percent-encoded query string, `w_rid` appended last — byte-identical
    /// to Python `enc_wbi`'s return value.
    pub fn query(&self) -> String {
        let mut query = form_urlencoded(&self.params);
        query.push_str("&w_rid=");
        query.push_str(&self.w_rid);
        query
    }
}

/// Percent-encode like `urllib.parse.urlencode` (space → `+`, unreserved
/// ASCII kept, everything else `%XX` per byte).
pub fn form_urlencoded(params: &[(String, String)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in params.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        encode_into(&mut out, k);
        out.push('=');
        encode_into(&mut out, v);
    }
    out
}

fn encode_into(out: &mut String, s: &str) {
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

fn clean_value(v: &str) -> String {
    v.chars().filter(|c| !STRIPPED.contains(c)).collect()
}

/// Sign `params` with the given keys and `wts` timestamp. Pure core of
/// Python `enc_wbi` after its `ensure_wbi_keys` gate.
pub fn enc_wbi(
    params: &[(String, String)],
    img_key: &str,
    sub_key: &str,
    wts: i64,
) -> Result<SignedParams, WbiError> {
    let key = mixin_key(img_key, sub_key)?;

    let mut all: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.clone(), clean_value(v)))
        .collect();
    all.push(("wts".to_owned(), wts.to_string()));
    all.sort_by(|a, b| a.0.cmp(&b.0));

    let query = form_urlencoded(&all);
    let digest = Md5::digest(format!("{query}{key}").as_bytes());
    let w_rid = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    Ok(SignedParams { params: all, w_rid })
}

/// Errors surfaced by the wbi stack.
#[derive(Debug, thiserror::Error)]
pub enum WbiError {
    /// Keys missing/short — Python's readable `WBI_KEY_UNAVAILABLE` path.
    #[error("wbi signature key unavailable")]
    KeysUnavailable,
    /// Lazy nav fetch failed (network or response without `wbi_img`).
    #[error("wbi signature key unavailable: nav fetch failed: {0}")]
    FetchFailed(String),
}

/// The `RequestSigner` plug from ticket 04: it sees each request's URL and
/// params, appends `wts` + `w_rid` when the URL is wbi-signed, and stays
/// out of the way otherwise (nav, playurl `pgc/pugv`, passport...).
///
/// Wraps a shared [`WbiKeyStore`] so every client clone signs with the
/// same keys and a nav refresh lands everywhere at once.
#[derive(Clone)]
pub struct WbiSigner {
    store: WbiKeyStore,
    signed_hosts: Arc<Vec<String>>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl WbiSigner {
    /// A signer that signs only Bilibili wbi API hosts (the `/wbi/`-style
    /// endpoints the Python build signs) and reads its clock from the
    /// system.
    pub fn new(store: WbiKeyStore) -> Self {
        Self::with_hosts(
            store,
            vec![
                "api.bilibili.com".to_owned(),
                "api.bilibili.com.".to_owned(),
            ],
            || {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or_default()
            },
        )
    }

    /// Test seam: explicit host gate and clock.
    pub fn with_hosts(
        store: WbiKeyStore,
        signed_hosts: Vec<String>,
        clock: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            store,
            signed_hosts: Arc::new(signed_hosts),
            clock: Arc::new(clock),
        }
    }

    fn host_is_signed(&self, url: &str) -> bool {
        let Some(rest) = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
        else {
            return false;
        };
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        self.signed_hosts
            .iter()
            .any(|h| host.eq_ignore_ascii_case(h))
    }
}

impl RequestSigner for WbiSigner {
    fn sign(&self, url: &str, params: &[(String, String)]) -> Vec<(String, String)> {
        if !self.host_is_signed(url) {
            return Vec::new();
        }
        let (img, sub) = self.store.keys();
        let wts = (self.clock)();
        match enc_wbi(params, &img, &sub, wts) {
            Ok(signed) => {
                let mut extra = signed.params;
                extra.push(("w_rid".to_owned(), signed.w_rid));
                extra
            }
            // Keys not ready: the Python build refuses to sign rather than
            // emit a doomed request; the caller's request goes out
            // unsigned and the server answers with its own error.
            Err(_) => Vec::new(),
        }
    }
}

/// Shared, lazily-refreshed img/sub key pair. Mirrors the Python store:
/// keys start empty, nav responses overwrite them, `save=False` means
/// nothing here ever touches disk.
#[derive(Clone, Default)]
pub struct WbiKeyStore {
    keys: Arc<RwLock<(String, String)>>,
    fetch_gate: Arc<tokio::sync::Mutex<Option<Instant>>>,
}

impl WbiKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_keys(img_key: &str, sub_key: &str) -> Self {
        Self {
            keys: Arc::new(RwLock::new((img_key.to_owned(), sub_key.to_owned()))),
            fetch_gate: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn keys(&self) -> (String, String) {
        self.keys.read().clone()
    }

    /// `wbi_keys_ready()` — combined length reaches `WBI_KEY_LENGTH`.
    pub fn ready(&self) -> bool {
        let (img, sub) = self.keys();
        img.chars().count() + sub.chars().count() >= WBI_KEY_LENGTH
    }

    /// `store_wbi_keys(data)` + `save=False`: overwrite from a nav
    /// response. Returns whether keys were obtained.
    pub fn store_from_nav(&self, data: &serde_json::Value) -> bool {
        match wbi_keys_from_nav(data) {
            Some((img, sub)) => {
                *self.keys.write() = (img, sub);
                true
            }
            None => false,
        }
    }

    /// `ensure_wbi_keys()`: single-flight lazy fetch (the caller's HTTP
    /// client does the GET), 30 s throttle (`FETCH_THROTTLE_SECONDS`),
    /// readiness re-checked under the lock so racing callers reuse the
    /// first one's result.
    pub async fn ensure(
        &self,
        fetch: impl Future<Output = Result<serde_json::Value, String>>,
    ) -> Result<(), WbiError> {
        if self.ready() {
            return Ok(());
        }
        let mut last_attempt = self.fetch_gate.lock().await;
        if self.ready() {
            return Ok(());
        }
        let now = Instant::now();
        if let Some(last) = *last_attempt {
            if now.duration_since(last) < Duration::from_secs_f32(FETCH_THROTTLE_SECONDS) {
                return Err(WbiError::KeysUnavailable);
            }
        }
        *last_attempt = Some(now);
        drop(last_attempt);

        let response = fetch.await.map_err(WbiError::FetchFailed)?;
        let data = response
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if self.store_from_nav(&data) {
            Ok(())
        } else {
            Err(WbiError::FetchFailed(
                "nav response carries no wbi_img".into(),
            ))
        }
    }
}

/// Python `FETCH_THROTTLE_SECONDS = 30.0`.
pub const FETCH_THROTTLE_SECONDS: f32 = 30.0;
