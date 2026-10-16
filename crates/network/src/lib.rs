//! Shared async HTTP layer for the Bili23 rewrite (ticket 04).
//!
//! Ports the observable behavior of the Python `util/network/request.py`:
//! one shared reqwest client with a cookie jar, rustls TLS, proxy
//! (none/system/manual), custom User-Agent, and per-request signing hooks.
//! Timeouts mirror the Python client: a generous connect timeout (the TLS
//! handshake is counted in by the stack; weak links need it) and a tight
//! read/write timeout so stuck responses fail fast.

pub mod cdn;
pub mod probe;

use std::sync::Arc;

use parking_lot::RwLock;
use reqwest::cookie::Jar;
use reqwest::header::{HeaderMap, HeaderValue, REFERER};
use reqwest::{Client, Proxy as ReqwestProxy, StatusCode, Url};

/// Connect timeout (Python `CONNECT_TIMEOUT`).
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// Read/write timeout (Python `READ_WRITE_TIMEOUT`).
pub const READ_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Default Referer sent on every request (Python `get_headers`).
pub const DEFAULT_REFERER: &str = "https://www.bilibili.com/";

/// How the client resolves proxies (`Advanced.proxy_mode` in config.json).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyMode {
    /// No proxy, and environment proxies are ignored.
    #[default]
    None,
    /// Honor system/environment proxy settings (httpx default behavior).
    System,
    /// Use the proxy server configured in the app.
    Manual,
}

/// Proxy protocol (Python only ships HTTP; enum kept 1:1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyType {
    #[default]
    Http,
}

/// Full proxy configuration, mirrors the Python `Proxy` object.
#[derive(Debug, Clone, Default)]
pub struct ProxyConfig {
    pub mode: ProxyMode,
    pub proxy_type: ProxyType,
    pub server: String,
    pub port: u16,
    pub uname: String,
    pub password: String,
}

impl ProxyConfig {
    /// Direct connection (proxy off).
    pub fn none() -> Self {
        Self {
            mode: ProxyMode::None,
            ..Self::default()
        }
    }
}

/// Per-request signing hook. Ticket 08 (wbi) plugs in here: it sees the
/// request's URL and params and returns additional params to append
/// (e.g. `w_rid` + `wts`).
pub trait RequestSigner: Send + Sync {
    fn sign(&self, url: &str, params: &[(String, String)]) -> Vec<(String, String)>;
}

/// A cookie entry persisted across restarts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

/// Construction options for [`HttpClient`].
#[derive(Default)]
pub struct ClientOptions {
    /// Value of `Advanced.user_agent` from config.json.
    pub user_agent: String,
    pub proxy: ProxyConfig,
    /// Cookies replayed from the previous run (config-persisted set).
    pub cookies_from_persistence: Vec<PersistedCookie>,
    /// Optional request signer (wbi).
    pub signer: Option<Arc<dyn RequestSigner>>,
}

/// Shared async HTTP client. Cloneable; all clones share one cookie jar.
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    jar: Arc<Jar>,
    cookies: Arc<RwLock<Vec<PersistedCookie>>>,
    signer: Option<Arc<dyn RequestSigner>>,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("signer", &self.signer.is_some())
            .finish_non_exhaustive()
    }
}

/// Manual-proxy construction failure is a config problem, not transport.
#[derive(Debug, thiserror::Error)]
#[error("invalid proxy configuration {proxy_url}: {source}")]
pub struct InvalidProxy {
    proxy_url: String,
    #[source]
    source: reqwest::Error,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("HTTP {status} from {url}")]
    Status { status: StatusCode, url: String },
    #[error("invalid JSON body: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    InvalidProxy(#[from] InvalidProxy),
}

impl HttpError {
    /// True if the error is a non-2xx HTTP status.
    pub fn is_status(&self) -> bool {
        matches!(self, HttpError::Status { .. })
    }
}

impl HttpClient {
    /// Builds a client per the options: rustls, cookie jar, proxy per mode,
    /// UA from config, shared referer header.
    pub fn new(options: ClientOptions) -> Result<Self, HttpError> {
        let mut builder = Client::builder()
            .user_agent(options.user_agent)
            .default_headers(default_headers())
            .timeout(READ_WRITE_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(10));

        match options.proxy.mode {
            ProxyMode::None => {
                builder = builder.no_proxy();
            }
            ProxyMode::System => { /* reqwest default: system + env proxies */ }
            ProxyMode::Manual => {
                builder = builder.proxy(manual_proxy(&options.proxy)?);
            }
        }

        let jar = Arc::new(Jar::default());
        builder = builder.cookie_provider(jar.clone());
        let client = builder.build()?;

        let cookies = Arc::new(RwLock::new(Vec::new()));
        for cookie in &options.cookies_from_persistence {
            set_cookie(&jar, cookie);
            cookies.write().push(cookie.clone());
        }

        Ok(Self {
            client,
            jar,
            cookies,
            signer: options.signer,
        })
    }

    /// Client tuned for CDN host probing: no signer, no cookie replay,
    /// tight timeouts. Probing must not touch the shared cookie jar
    /// (CDN auth lives in query params) and must fail a dead node fast.
    /// `user_agent` and `proxy` come from the same options as
    /// [`HttpClient::new`].
    pub fn probe(options: ClientOptions) -> Result<Self, HttpError> {
        let mut builder = Client::builder()
            .user_agent(options.user_agent)
            .default_headers(default_headers())
            .timeout(crate::probe::PROBE_TIMEOUT)
            .connect_timeout(crate::probe::PROBE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(10))
            .pool_max_idle_per_host(16);

        match options.proxy.mode {
            ProxyMode::None => {
                builder = builder.no_proxy();
            }
            ProxyMode::System => {}
            ProxyMode::Manual => {
                builder = builder.proxy(manual_proxy(&options.proxy)?);
            }
        }

        Ok(Self {
            client: builder.build()?,
            jar: Arc::new(Jar::default()),
            cookies: Arc::new(RwLock::new(Vec::new())),
            signer: None,
        })
    }

    /// Signed GET returning the response body parsed as JSON.
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
    ) -> Result<T, HttpError> {
        let body = self.get(url, params).await?;
        Ok(serde_json::from_str(&body)?)
    }

    /// Signed GET returning the response body as text.
    pub async fn get(&self, url: &str, params: &[(String, String)]) -> Result<String, HttpError> {
        let mut all = params.to_vec();
        if let Some(signer) = &self.signer {
            all.extend(signer.sign(url, params));
        }
        let mut request = self.client.get(url);
        if !all.is_empty() {
            request = request.query(&all);
        }
        let response = request.send().await?;
        check_status(response, url).await
    }

    /// Signed POST with a JSON body, returning the body parsed as JSON.
    pub async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
        json: &impl serde::Serialize,
    ) -> Result<T, HttpError> {
        let body = self.post_json_body(url, params, json).await?;
        Ok(serde_json::from_str(&body)?)
    }

    /// Signed POST with a JSON body, returning the raw response text.
    pub async fn post_json_body(
        &self,
        url: &str,
        params: &[(String, String)],
        json: &impl serde::Serialize,
    ) -> Result<String, HttpError> {
        let mut all = params.to_vec();
        if let Some(signer) = &self.signer {
            all.extend(signer.sign(url, params));
        }
        let mut request = self.client.post(url).json(json);
        if !all.is_empty() {
            request = request.query(&all);
        }
        let response = request.send().await?;
        check_status(response, url).await
    }

    /// Installs a cookie into the shared jar and the persisted set.
    ///
    /// Mirrors Python `_apply_cookies`: login/anonymous provisioning writes
    /// cookies with `domain=".bilibili.com", path="/"`.
    pub fn set_cookie(&self, name: &str, value: &str, domain: &str, path: &str) {
        let cookie = PersistedCookie {
            name: name.to_owned(),
            value: value.to_owned(),
            domain: domain.to_owned(),
            path: path.to_owned(),
        };
        set_cookie(&self.jar, &cookie);
        let mut cookies = self.cookies.write();
        cookies.retain(|c| c.name != cookie.name);
        cookies.push(cookie);
    }

    /// Snapshot of the persisted cookie set (what a restart would replay).
    pub fn persisted_cookie_snapshot(&self) -> Vec<PersistedCookie> {
        self.cookies.read().clone()
    }

    /// Read access to the inner reqwest client for downloads/ranges.
    pub fn inner(&self) -> &Client {
        &self.client
    }
}

fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(REFERER, HeaderValue::from_static(DEFAULT_REFERER));
    headers
}

fn set_cookie(jar: &Jar, cookie: &PersistedCookie) {
    // The Domain attribute makes it a domain cookie (matches subdomains,
    // like Python's cookies.set(domain=".bilibili.com")); the URL scopes
    // which host the store accepts it for.
    let host = cookie.domain.trim_start_matches('.');
    let base = format!("https://{host}/");
    let Ok(url) = Url::parse(&base) else {
        return;
    };
    let header = format!(
        "{}={}; Domain={}; Path={}",
        cookie.name, cookie.value, cookie.domain, cookie.path
    );
    jar.add_cookie_str(&header, &url);
}

fn manual_proxy(config: &ProxyConfig) -> Result<ReqwestProxy, InvalidProxy> {
    let scheme = match config.proxy_type {
        ProxyType::Http => "http",
    };
    let auth = if !config.uname.is_empty() {
        format!("{}:{}@", config.uname, config.password)
    } else {
        String::new()
    };
    let proxy_url = format!("{scheme}://{auth}{}:{}", config.server, config.port);
    ReqwestProxy::all(&proxy_url).map_err(|source| InvalidProxy {
        proxy_url,
        source,
    })
}

async fn check_status(response: reqwest::Response, url: &str) -> Result<String, HttpError> {
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(HttpError::Status {
            status,
            url: url.to_owned(),
        });
    }
    Ok(text)
}
