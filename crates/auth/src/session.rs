//! Anonymous session provisioning + persisted login state — port of
//! `util/auth/cookie.py` (`CookieManager`) and the `Cookie` section of
//! config.json.
//!
//! The Rust build keeps this self-contained (plain struct + JSON) until
//! ticket 02's config crate lands; field names match config.json's
//! `Cookie` section 1:1 so adoption is a type swap.

use std::path::{Path, PathBuf};

use digest::{FixedOutput, KeyInit, Update};
use hmac::SimpleHmac;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::murmur3::buvid_fp_hex;
use network::HttpClient;

/// `https://api.bilibili.com/x/frontend/finger/spi`.
pub const FINGER_SPI_API: &str = "https://api.bilibili.com/x/frontend/finger/spi";
/// `https://api.bilibili.com/bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket`.
pub const GEN_WEB_TICKET_API: &str =
    "https://api.bilibili.com/bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket";

/// Cookie domain for everything this crate writes (Python
/// `_apply_cookies`). Use the wiremock host in tests.
pub const BILIBILI_DOMAIN: &str = ".bilibili.com";

const TICKET_HMAC_KEY: &[u8] = b"XgwSnGZ1p";
const BUVID_FP_SEED: u64 = 31;

/// Persisted auth state — config.json's `Cookie` section (plus the wbi
/// keys Python stores session-scoped; they round-trip here so the file
/// shape stays one section).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthState {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub buvid3: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub buvid4: String,
    #[serde(default)]
    pub buvid_expires: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub b_lsid: String,
    #[serde(default)]
    pub b_nut: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub buvid_fp: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bili_ticket: String,
    #[serde(default)]
    pub bili_ticket_expires: i64,
    #[serde(default)]
    pub is_login: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bili_jct: String,
    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "DedeUserID"
    )]
    pub dede_user_id: String,
    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "DedeUserID__ckMd5"
    )]
    pub dede_user_id_ck_md5: String,
    #[serde(default, skip_serializing_if = "String::is_empty", rename = "SESSDATA")]
    pub sessdata: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub img_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sub_key: String,
}

impl AuthState {
    /// Read the `Cookie` section out of a config.json.
    pub fn from_config_file(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let root: serde_json::Value = serde_json::from_str(&text).ok()?;
        serde_json::from_value(root.get("Cookie")?.clone()).ok()
    }

    /// Write back into the config.json's `Cookie` key, preserving every
    /// other section untouched.
    pub fn save_to_config_file(&self, path: &Path) -> std::io::Result<()> {
        let mut root: serde_json::Value = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        };
        root["Cookie"] = serde_json::to_value(self).expect("AuthState serializes");
        let text = serde_json::to_string_pretty(&root)?;
        std::fs::write(path, text)
    }
}

/// `get_uuid` — parts of [8,4,4,4,12] over the charset
/// `"123456789ABCDEF"` + the 2-char literal `"10"`, then
/// `{t % 100000}` zero-padded to 5 + `"infoc"`.
pub fn generate_uuid(now: i64, rng: &mut impl RngExt) -> String {
    let charset: Vec<&str> = vec![
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "10",
    ];
    let mut parts = Vec::with_capacity(5);
    for &len in &[8usize, 4, 4, 4, 12] {
        let mut part = String::with_capacity(len + 1);
        for _ in 0..len {
            part.push_str(charset[rng.random_range(0..charset.len())]);
        }
        parts.push(part);
    }
    let t = format!("{:0>5}", now % 100_000);
    format!("{}{}infoc", parts.join("-"), t)
}

/// `get_b_lsid` — 8 uppercase hex digits, `_`, timestamp as upper hex.
pub fn generate_b_lsid(now: i64, rng: &mut impl RngExt) -> String {
    let mut ret = String::with_capacity(9);
    for _ in 0..8 {
        let digit: u8 = rng.random_range(0..16);
        ret.push_str(&format!("{digit:X}"));
    }
    format!("{ret}_{now:X}")
}

/// `GenWebTicket` `hexsign` — HMAC-SHA256(key="XgwSnGZ1p",
/// msg="ts{ts}") hex digest.
pub fn gen_web_ticket_hexsign(ts: i64) -> String {
    let mut mac =
        SimpleHmac::<Sha256>::new_from_slice(TICKET_HMAC_KEY).expect("HMAC accepts any key length");
    mac.update(format!("ts{ts}").as_bytes());
    let digest = mac.finalize_fixed();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Python `get_timestamp_next_day` — start of the next local day.
/// Implemented over UTC days; the only consumer compares it to a wall
/// clock to decide whether to re-fetch buvid, where the exact TZ shift
/// just moves the refresh by a few hours.
pub fn timestamp_next_day(now: i64) -> i64 {
    let secs_per_day = 86_400;
    (now / secs_per_day + 1) * secs_per_day
}

/// Errors from the provisioning chain and login flows.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("request failed: {0}")]
    Http(#[from] network::HttpError),
    /// Response-level failure (`code != 0`) — Python `check_response`
    /// raising `RuntimeError(message)`.
    #[error("请求失败: {0}")]
    Response(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

fn check_response(response: &serde_json::Value) -> Result<(), AuthError> {
    let code = response.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        let message = response
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(AuthError::Response(message.to_owned()));
    }
    Ok(())
}

/// Drives the anonymous provisioning chain against an [`HttpClient`],
/// persisting into `state` and installing cookies as it goes.
///
/// Order mirrors `CookieManager`: `get_buvid` (skipped while unexpired) →
/// on success `get_uuid`/`get_b_lsid`/`get_b_nut`/`get_buvid_fp` → cookie
/// sync → `exclimbwuzhi`; `get_bili_ticket` (skipped while unexpired)
/// runs independently.
pub struct Session<'a> {
    pub client: &'a HttpClient,
    pub state: &'a mut AuthState,
    pub user_agent: String,
    pub domain: &'a str,
    /// Test seam: fixed clock (seconds).
    pub now: i64,
    /// Test seam: fixed clock (ms) for the ExClimbWuzhi payload.
    pub now_ms: i64,
    /// Test seam: deterministic entropy for uuid/b_lsid.
    pub rng_seed: u64,
}

impl Session<'_> {
    /// `init_cookie_info` — the full chain, best-effort per step in
    /// Python (each failure toasts and moves on); here the first
    /// transport error aborts the chain because later steps depend on
    /// earlier cookies.
    pub async fn init(&mut self) -> Result<(), AuthError> {
        let refresh_buvid = self.now >= self.state.buvid_expires;
        if refresh_buvid {
            self.get_buvid().await?;
        }
        if self.now >= self.state.bili_ticket_expires {
            self.get_bili_ticket().await?;
        }
        self.exclimbwuzhi().await
    }

    /// `get_buvid` + its success chain.
    pub async fn get_buvid(&mut self) -> Result<(), AuthError> {
        let response: serde_json::Value = self
            .client
            .get_json::<serde_json::Value>(FINGER_SPI_API, &[])
            .await?;
        check_response(&response)?;
        let data = &response["data"];
        self.state.buvid3 = data["b_3"].as_str().unwrap_or_default().to_owned();
        self.state.buvid4 = data["b_4"].as_str().unwrap_or_default().to_owned();
        self.state.buvid_expires = timestamp_next_day(self.now);

        let mut rng = make_rng(self.rng_seed);
        self.state.uuid = generate_uuid(self.now, &mut rng);
        self.state.b_lsid = generate_b_lsid(self.now, &mut rng);
        self.state.b_nut = self.now;
        self.state.buvid_fp = buvid_fp_hex(&self.user_agent, BUVID_FP_SEED);

        self.sync_anonymous_cookies();
        Ok(())
    }

    /// `get_bili_ticket` via `GenWebTicket` — POST, params in query, no
    /// body (Python passes only `params=`).
    pub async fn get_bili_ticket(&mut self) -> Result<(), AuthError> {
        let params = vec![
            ("key_id".to_owned(), "ec02".to_owned()),
            ("hexsign".to_owned(), gen_web_ticket_hexsign(self.now)),
            ("context[ts]".to_owned(), self.now.to_string()),
            ("csrf".to_owned(), self.state.bili_jct.clone()),
        ];
        let response: serde_json::Value =
            post_params(self.client, GEN_WEB_TICKET_API, &params).await?;
        check_response(&response)?;
        self.state.bili_ticket = response["data"]["ticket"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        self.state.bili_ticket_expires = self.now + 3 * 24 * 3600;
        self.sync_anonymous_cookies();
        Ok(())
    }

    /// `exclimbwuzhi` — activate buvid3. The body is the exact
    /// `dumps_python` string (stdlib separators), so it must bypass
    /// serde_json re-serialization.
    pub async fn exclimbwuzhi(&mut self) -> Result<(), AuthError> {
        let payload = crate::exclimbwuzhi::exclimbwuzhi_payload(
            &self.user_agent,
            &self.state.uuid,
            self.now_ms,
        );
        let response = self
            .client
            .inner()
            .post(crate::exclimbwuzhi::EXCLIMBWUZHI_API)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await
            .map_err(network::HttpError::from)?
            .text()
            .await
            .map_err(network::HttpError::from)?;
        let response: serde_json::Value =
            serde_json::from_str(&response).map_err(network::HttpError::Json)?;
        check_response(&response)
    }

    /// `update_cookies` — install the anonymous set (login cookies are
    /// installed by the login flows' `capture_login_cookies`).
    pub fn sync_anonymous_cookies(&self) {
        let entries: Vec<(&str, String)> = vec![
            ("_uuid", self.state.uuid.clone()),
            ("b_lsid", self.state.b_lsid.clone()),
            ("b_nut", self.state.b_nut.to_string()),
            ("bili_ticket", self.state.bili_ticket.clone()),
            (
                "bili_ticket_expires",
                self.state.bili_ticket_expires.to_string(),
            ),
            ("buvid_fp", self.state.buvid_fp.clone()),
            ("buvid3", self.state.buvid3.clone()),
            ("buvid4", self.state.buvid4.clone()),
            ("CURRENT_FNVAL", "4048".to_owned()),
            ("CURRENT_QUALITY", "0".to_owned()),
        ];
        for (name, value) in entries {
            self.client.set_cookie(name, &value, self.domain, "/");
        }
        if self.state.is_login {
            self.sync_login_cookies();
        }
    }

    /// Login-only cookies (`get_cookies`' `is_login` branch).
    pub fn sync_login_cookies(&self) {
        let entries: Vec<(&str, String)> = vec![
            ("bili_jct", self.state.bili_jct.clone()),
            ("DedeUserID", self.state.dede_user_id.clone()),
            ("DedeUserID__ckMd5", self.state.dede_user_id_ck_md5.clone()),
            ("SESSDATA", self.state.sessdata.clone()),
        ];
        for (name, value) in entries {
            self.client.set_cookie(name, &value, self.domain, "/");
        }
    }

    /// Python `update_cookies` after login success: snapshot the client
    /// jar and persist the four login cookies + `is_login=true`.
    pub fn capture_login_cookies(&mut self) {
        let jar = self.client.persisted_cookie_snapshot();
        let get = |name: &str| {
            jar.iter()
                .find(|c| c.name == name)
                .map(|c| c.value.clone())
                .unwrap_or_default()
        };
        self.state.bili_jct = get("bili_jct");
        self.state.dede_user_id = get("DedeUserID");
        self.state.dede_user_id_ck_md5 = get("DedeUserID__ckMd5");
        self.state.sessdata = get("SESSDATA");
        self.state.is_login = true;
    }

    /// Logout success path: clear login fields.
    pub fn clear_login(&mut self) {
        self.state.is_login = false;
        self.state.bili_jct.clear();
        self.state.dede_user_id.clear();
        self.state.dede_user_id_ck_md5.clear();
        self.state.sessdata.clear();
    }
}

/// POST with params in the query string and no body — the httpx shape
/// Python uses for `GenWebTicket`/SMS/login/logout (they pass only
/// `params=`). Not wbi-signed (none of these hosts are in the signer's
/// gate), and response Set-Cookies still land in the shared jar via the
/// inner client.
pub async fn post_params(
    client: &HttpClient,
    url: &str,
    params: &[(String, String)],
) -> Result<serde_json::Value, AuthError> {
    let mut request = client.inner().post(url);
    if !params.is_empty() {
        request = request.query(params);
    }
    let text = request
        .send()
        .await
        .map_err(network::HttpError::from)?
        .text()
        .await
        .map_err(network::HttpError::from)?;
    Ok(serde_json::from_str(&text).map_err(network::HttpError::Json)?)
}

fn make_rng(seed: u64) -> rand::rngs::StdRng {
    use rand::SeedableRng;
    // ponytail: seeded StdRng mirrors Python's random for tests; the
    // production call site passes a real entropy source's seed.
    rand::rngs::StdRng::seed_from_u64(seed)
}

/// Path helper for tests and the future app glue: the config.json path
/// inside a user data dir.
pub fn config_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("config.json")
}
