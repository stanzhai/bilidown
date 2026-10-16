//! Parse dispatcher: the async core behind the Parse button (port of
//! `src/util/parse/worker.py` + the parse handlers in
//! `src/gui/interface/parse.py`).
//!
//! One pure async function ([`parse`]) routes a classified url to its
//! parser family. The UI layer owns generation tokens, progress tips and
//! result routing; this module never touches gpui so it tests headless
//! against a wiremock server.

use std::future::Future;
use std::sync::Arc;

use network::HttpClient;
use parsers::list::{self, ListError, ListParseContext, ParseOutput};
use parsers::media_parser::{self, ParseOutcome};
use parsers::tree::TreeItem;
use serde_json::Value;
use urls::classify::ParserType as P;
#[allow(dead_code)] // wired in by the shell parse runner (next commit)
pub const BASE_URL: &str = "https://api.bilibili.com";

#[allow(dead_code)] // wired in by the shell parse runner (next commit)
pub const WBI_WAIT_TIMEOUT_MS: u64 = 65_000;
#[allow(dead_code)] // wired in by the shell parse runner (next commit)
pub const WBI_WAIT_POLL_MS: u64 = 500;
#[allow(dead_code)] // wired in by the shell parse runner (next commit)
pub const WBI_FETCH_KICK_TICKS: u32 = 10;
const MAX_EXPANSIONS: u32 = 3;

/// Inputs the parse routers need beyond the url: the login session
/// (watch-later/history/lesson need it) and the requested list page.
pub struct ParseRequest<'a> {
    pub url: &'a str,
    pub pn: i64,
    pub auth: &'a auth::AuthState,
}

/// Route one url to its parser family. Redirect chains (b23 → video,
/// video → video) are followed, bounded.
pub async fn parse(
    client: &HttpClient,
    ctx: &ListParseContext,
    req: &ParseRequest<'_>,
) -> Result<ParseOutput, ParseError> {
    let mut url = req.url.to_string();
    let mut expansions = 0;
    loop {
        let Some(classified) = urls::classify::classify(&url) else {
            return Err(ParseError::InvalidLink(url));
        };
        match classified.parser_type {
            P::B23 | P::Festival => {
                if expansions >= MAX_EXPANSIONS {
                    return Err(ParseError::RedirectLoop(url));
                }
                expansions += 1;
                url = urls::expand::expand_b23(client.inner(), &url)
                    .await
                    .map_err(ParseError::Expand)?;
            }
            P::Video => {
                let mut current = url.clone();
                for _ in 0..MAX_EXPANSIONS {
                    match media_parser::parse_video_url(client, BASE_URL, &current, &ctx.episode_data)
                        .await?
                    {
                        ParseOutcome::Redirect(next) => current = next,
                        ParseOutcome::Episode(node, (_key, value)) => {
                            ctx.episode_data.set_episode_data(&node.episode_id, value);
                            return Ok(one_shot("USER_UPLOADS", node, None));
                        }
                        // Ticket 25 owns the interactive dialog; until
                        // then the walked tree returns as-is.
                        ParseOutcome::Interactive(node) => {
                            return Ok(one_shot("INTERACTIVE_VIDEO", node, None));
                        }
                    }
                }
                return Err(ParseError::RedirectLoop(current));
            }
            P::Bangumi => {
                let (node, current) =
                    media_parser::parse_bangumi_url(client, BASE_URL, &url, &ctx.episode_data)
                        .await?;
                return Ok(one_shot("ANIME", node, current));
            }
            P::Cheese => {
                let (node, current) =
                    media_parser::parse_cheese_url(client, BASE_URL, &url, &ctx.episode_data)
                        .await?;
                return Ok(one_shot("COURSE", node, current));
            }
            P::Lesson => {
                let (node, current) = media_parser::parse_lesson_url(
                    client,
                    BASE_URL,
                    &url,
                    req.auth,
                    &ctx.episode_data,
                )
                .await?;
                return Ok(one_shot("MALL_COURSE", node, current));
            }
            P::Audio => {
                let info = parsers::audio::parse_audio(client, BASE_URL, &url).await?;
                let node = parsers::audio::build_audio_tree(&info.data);
                return Ok(one_shot("AUDIO", node, None));
            }
            P::List => {
                return Ok(list::parse_list(client, BASE_URL, ctx, &url, req.pn).await?)
            }
            P::Favlist => {
                return Ok(list::parse_favlist(client, BASE_URL, ctx, &url, req.pn).await?)
            }
            P::Space => {
                return Ok(list::parse_space(client, BASE_URL, ctx, &url, req.pn).await?)
            }
            P::Popular => return Ok(list::parse_popular(client, BASE_URL, ctx, &url).await?),
            P::WatchLater => {
                return Ok(list::parse_watch_later(client, BASE_URL, ctx, &url, req.auth, req.pn)
                    .await?)
            }
            P::History => {
                return Ok(list::parse_history(client, BASE_URL, ctx, &url, req.auth, req.pn)
                    .await?)
            }
        }
    }
}

fn one_shot(
    category: &'static str,
    node: TreeItem,
    current_episode: Option<(String, String)>,
) -> ParseOutput {
    ParseOutput {
        category,
        node,
        extra: list::ExtraData::default(),
        current_episode,
    }
}

/// Parse failure surfaced to the "Parse Failed" toast.
#[derive(Debug)]
pub enum ParseError {
    InvalidLink(String),
    RedirectLoop(String),
    Expand(urls::expand::ExpandError),
    List(ListError),
    Media(media_parser::MediaError),
    Audio(parsers::audio::AudioError),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLink(url) => write!(f, "Invalid link: {url}"),
            Self::RedirectLoop(url) => write!(f, "Too many redirects: {url}"),
            Self::Expand(e) => write!(f, "{e}"),
            Self::List(e) => write!(f, "{e}"),
            Self::Media(e) => write!(f, "{e}"),
            Self::Audio(e) => write!(f, "{e}"),
        }
    }
}

impl From<ListError> for ParseError {
    fn from(e: ListError) -> Self {
        Self::List(e)
    }
}

impl From<media_parser::MediaError> for ParseError {
    fn from(e: media_parser::MediaError) -> Self {
        Self::Media(e)
    }
}

impl From<parsers::audio::AudioError> for ParseError {
    fn from(e: parsers::audio::AudioError) -> Self {
        Self::Audio(e)
    }
}

/// The long-lived parse state the shell owns: one HTTP client (WBI signer
/// wired in) plus the shared episode-data table.
pub struct ParseService {
    pub client: Arc<HttpClient>,
    pub ctx: ListParseContext,
    pub key_store: auth::WbiKeyStore,
}

impl ParseService {
    /// Production service from config: UA from `Advanced.user_agent`,
    /// WBI signer signing `api.bilibili.com`.
    pub fn new(cfg: &config::ConfigStore) -> Self {
        let key_store = auth::WbiKeyStore::new();
        let signer = Arc::new(auth::WbiSigner::new(key_store.clone()));
        let ua = cfg
            .get("Advanced", "user_agent")
            .and_then(Value::as_str)
            .unwrap_or("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0")
            .to_string();
        let client = HttpClient::new(network::ClientOptions {
            user_agent: ua,
            proxy: network::ProxyConfig::none(),
            cookies_from_persistence: Vec::new(),
            signer: Some(signer),
        })
        .expect("rustls client builds with a valid UA");
        Self {
            client: Arc::new(client),
            ctx: ListParseContext::new(),
            key_store,
        }
    }

    /// Whether the url's family needs WBI signing (`needs_wbi_signature`).
    pub fn needs_wbi(&self, url: &str) -> Option<bool> {
        urls::classify::parser_type(url)
            .map(|t| parsers::worker::needs_wbi_signature(t.key()).unwrap_or(false))
    }

    /// Poll until the key store is ready (bounded by
    /// [`WBI_WAIT_TIMEOUT_MS`]), kicking a single-flight fetch every
    /// [`WBI_FETCH_KICK_TICKS`] polls. `fetch_nav` GETs `x/web-interface/nav`.
    /// Port of `wait_for_wbi_keys` / `_on_wbi_wait_tick`.
    pub async fn wait_for_wbi_keys<F, Fut>(&self, fetch_nav: F) -> bool
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<Value, String>>,
    {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(WBI_WAIT_TIMEOUT_MS);
        let poll = std::time::Duration::from_millis(WBI_WAIT_POLL_MS);
        let mut ticks: u32 = 0;
        loop {
            if self.key_store.ready() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            ticks += 1;
            if ticks.is_multiple_of(WBI_FETCH_KICK_TICKS) {
                let _ = self.key_store.ensure(fetch_nav()).await;
            }
            tokio::time::sleep(poll).await;
        }
    }
}
