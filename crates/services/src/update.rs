//! Verhub update service (ticket 27): port of `src/util/misc/update.py`
//! plus the wire behavior of `verhub_sdk` 0.2.10 (the Python build calls it
//! through `VerhubClient.public.check_update`).
//!
//! Identity (spec Maturity acknowledgment): same base URL, same project key,
//! same `Bili23-Downloader/<version>` UA suffix the SDK appends. The update
//! *dialog* is ticket 25 (`app/ui/dialogs/update.rs`); it consumes
//! [`UpdateInfo`] from here.
//!
//! The client never touches the Bilibili cookie jar: like Python
//! `_create_client`, requests go out with no cookies (build a fresh
//! [`VerhubClient`] per check; proxies are resolved by the caller when
//! constructing the [`network::HttpClient`]).

use std::time::{SystemTime, UNIX_EPOCH};

use network::HttpClient;
use serde::Deserialize;

/// Verhub instance the Python build ships against (`update.py:14`).
pub const VERHUB_BASE_URL: &str = "https://verhub.hanloth.cn";

/// Update channel key; product identity, must not change (spec).
pub const VERHUB_PROJECT_KEY: &str = "scottsloan-bili23-downloader";

/// App version of the running build (`config.py app_version` line).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Comparable version the update service compares against
/// (`config.py app_comparable_version`, "2.20.0" in the 2.21.0 line).
pub const APP_COMPARABLE_VERSION: &str = "2.20.0";

/// User-Agent the SDK derives: its own token plus the app identifier
/// (`_http.py` user_agent/app_identifier). The verhub-sdk-python token is
/// kept so server-side statistics keep grouping this client with the
/// Python build's.
pub const USER_AGENT_PREFIX: &str = "verhub-sdk-python/0.2.10";

const PLATFORM_HEADER: &str = "x-verhub-platform";
const PLATFORM_VERSION_HEADER: &str = "x-verhub-platform-version";
const CLIENT_TIME_HEADER: &str = "x-verhub-client-time";

/// Locale tags Verhub accepts for this project (`update.py:17-19`).
const LOCALE_ZH_CN: &str = "zh-CN";
const LOCALE_ZH_TW: &str = "zh-TW";
const LOCALE_EN_US: &str = "en-US";

/// Region/script subtags that mean Traditional Chinese (`update.py:21`).
const TRADITIONAL_CHINESE_SUBTAGS: [&str; 4] = ["tw", "hk", "mo", "hant"];

/// Error from a Verhub request: non-2xx response with the server's
/// message, or transport failure (SDK `VerhubApiError`/`VerhubConnectionError`).
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("[{status}] {message}")]
    Api { status: u16, message: String },
    #[error("request {method} {url} failed: {source}")]
    Connection {
        method: String,
        url: String,
        #[source]
        source: network::HttpError,
    },
    #[error("invalid JSON body: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<network::HttpError> for UpdateError {
    fn from(e: network::HttpError) -> Self {
        match e {
            network::HttpError::Status { status, url } => UpdateError::Api {
                status: status.as_u16(),
                message: format!("HTTP {status} from {url}"),
            },
            other => UpdateError::Connection {
                method: String::new(),
                url: String::new(),
                source: other,
            },
        }
    }
}

/// The fields the update UI consumes — Python `parse_response` output
/// (`update.py:92-102`), plus the version title the dialog renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub should_update: bool,
    /// Forced update: dialog becomes unclosable, skip button hidden.
    pub required: bool,
    pub version: String,
    pub title: Option<String>,
    /// Release notes; empty string when the server has none.
    pub content: String,
    /// Download page URL; empty string when the version has none.
    pub update_url: String,
}

/// Machine-readable reasons attached to a check response
/// (`CheckUpdateResponse.reason_codes`); not consumed by the UI today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckUpdateOutcome {
    pub info: UpdateInfo,
    pub reason_codes: Vec<String>,
}

/// `to_verhub_locale` (`update.py:37-56`): map the interface language to
/// the locale tag sent to Verhub. Accepts config.json literals
/// (`zh_CN`/`zh_TW`/`en_US`/`Auto`), Qt `QLocale.name()` (`zh_HK`) and
/// BCP-47 tags (`zh-Hant-TW`, `en`). `Auto`/empty → `None` (no preference,
/// server default content); Chinese with a Traditional subtag → `zh-TW`,
/// anything else → `en-US`.
pub fn to_verhub_locale(language: Option<&str>) -> Option<&'static str> {
    let tag = language
        .unwrap_or_default()
        .trim()
        .replace('_', "-")
        .to_lowercase();
    if tag.is_empty() || tag == "auto" {
        return None;
    }

    let subtags: Vec<&str> = tag.split('-').collect();
    if subtags[0] == "zh" {
        if subtags[1..]
            .iter()
            .any(|s| TRADITIONAL_CHINESE_SUBTAGS.contains(s))
        {
            Some(LOCALE_ZH_TW)
        } else {
            Some(LOCALE_ZH_CN)
        }
    } else {
        Some(LOCALE_EN_US)
    }
}

/// `parse_response` (`update.py:92-102`): flatten the check response into
/// the UI fields, defaulting missing notes/URL to empty strings.
pub fn parse_response(response: &serde_json::Value) -> UpdateInfo {
    let latest = &response["latest_version"];
    UpdateInfo {
        should_update: response["should_update"].as_bool().unwrap_or(false),
        required: response["required"].as_bool().unwrap_or(false),
        version: latest["version"].as_str().unwrap_or_default().to_owned(),
        title: latest["title"].as_str().map(str::to_owned),
        content: latest["content"].as_str().unwrap_or_default().to_owned(),
        update_url: latest["download_url"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    }
}

/// Parameters of one check — everything `Updater.request_update` assembles
/// (`update.py:195-200`): current versions from `config.py`, preview flag
/// from `Update.include_prerelease`, locale from the interface language.
#[derive(Debug, Clone)]
pub struct CheckUpdateParams {
    pub current_version: String,
    pub current_comparable_version: String,
    pub include_preview: bool,
    pub locale: Option<String>,
}

impl Default for CheckUpdateParams {
    fn default() -> Self {
        Self {
            current_version: APP_VERSION.to_owned(),
            current_comparable_version: APP_COMPARABLE_VERSION.to_owned(),
            include_preview: false,
            locale: None,
        }
    }
}

/// Skip-version gate (`Updater.check`, `update.py:173-178`): when the check
/// is automatic and the offered version is the skipped one, suppress the
/// prompt. `manual` rides with the request, not shared state — automatic
/// (startup) and manual (settings page) checks can be in flight at once.
pub fn suppresses_prompt(info: &UpdateInfo, skip_version: &str, manual: bool) -> bool {
    info.should_update && !manual && !skip_version.is_empty() && skip_version == info.version
}

/// Verhub client: URL scope + headers over a shared [`HttpClient`].
///
/// Cheap to build per check (Python builds one per check too — proxies can
/// change at runtime and the check is infrequent).
pub struct VerhubClient<'a> {
    http: &'a HttpClient,
    base_url: String,
    user_agent: String,
    platform: &'static str,
    platform_version: Option<String>,
}

impl<'a> VerhubClient<'a> {
    /// Client for this build's channel: UA is the SDK token plus
    /// `Bili23-Downloader/<version>` (`update.py:85`).
    pub fn new(http: &'a HttpClient) -> Self {
        Self::with_options(http, VERHUB_BASE_URL, VERHUB_PROJECT_KEY, APP_VERSION)
    }

    /// Testable core: explicit base URL (wiremock origin), project key and
    /// app version. No proxy or cookie setup — the caller owns the
    /// [`HttpClient`].
    pub fn with_options(
        http: &'a HttpClient,
        base_url: &str,
        project_key: &str,
        app_version: &str,
    ) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            user_agent: format!("{USER_AGENT_PREFIX} Bili23-Downloader/{app_version}"),
            platform: detect_platform(),
            platform_version: detect_platform_version(),
        }
        .with_project_key(project_key)
    }

    fn with_project_key(mut self, project_key: &str) -> Self {
        // The key is a fixed identifier, but it still goes through the same
        // URL encoding the SDK applies (`_resolve_path` quote).
        self.base_url = format!(
            "{}/api/v1/public/{}",
            self.base_url,
            urlencode_component(project_key)
        );
        self
    }

    async fn get(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<serde_json::Value, UpdateError> {
        self.request("GET", path, query, None).await
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, UpdateError> {
        let mut url = format!("{}{}", self.base_url, path);
        let pairs: Vec<(String, String)> = query
            .iter()
            .filter(|(_, v)| !v.is_empty()) // SDK drops None params
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        if !pairs.is_empty() {
            let qs: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("{}={}", urlencode_component(k), urlencode_component(v)))
                .collect();
            url.push('?');
            url.push_str(&qs.join("&"));
        }

        let mut request = self.http.inner().request(
            reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
            &url,
        );
        request = request
            .header("Accept", "application/json")
            .header("User-Agent", &self.user_agent)
            .header(PLATFORM_HEADER, self.platform)
            .header(CLIENT_TIME_HEADER, format_client_time());
        if let Some(v) = &self.platform_version {
            request = request.header(PLATFORM_VERSION_HEADER, v);
        }
        if let Some(json) = body {
            request = request.json(json);
        }

        let response = request.send().await.map_err(|e| UpdateError::Connection {
            method: method.to_owned(),
            url: url.clone(),
            source: network::HttpError::from(e),
        })?;

        let status = response.status();
        let text = response.text().await.map_err(|e| UpdateError::Connection {
            method: method.to_owned(),
            url: url.clone(),
            source: network::HttpError::from(e),
        })?;

        // SDK `_parse_json`: empty body → {}, non-JSON stays raw text
        // (error path keeps the payload readable).
        let data: serde_json::Value = if text.is_empty() {
            serde_json::Value::Object(Default::default())
        } else {
            serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone()))
        };

        if !status.is_success() {
            return Err(UpdateError::Api {
                status: status.as_u16(),
                message: error_message(&data)
                    .unwrap_or_else(|| format!("HTTP {}", status.as_u16())),
            });
        }
        Ok(data)
    }

    /// `public.check_update` (`public_api.py:119-156`): POST the current
    /// version pair; `include_preview`/`locale` omitted when unset
    /// (SDK `compact` drops UNSET fields — the server defaults them).
    pub async fn check_update(
        &self,
        params: &CheckUpdateParams,
    ) -> Result<CheckUpdateOutcome, UpdateError> {
        let mut body = serde_json::json!({
            "current_version": params.current_version,
            "current_comparable_version": params.current_comparable_version,
        });
        if params.include_preview {
            body["include_preview"] = serde_json::Value::Bool(true);
        }
        if let Some(locale) = &params.locale {
            body["locale"] = serde_json::Value::String(locale.clone());
        }

        let value = self
            .request("POST", "/versions/check-update", &[], Some(&body))
            .await?;
        let reason_codes = value["reason_codes"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(CheckUpdateOutcome {
            info: parse_response(&value),
            reason_codes,
        })
    }

    /// `public.list_versions` (`public_api.py:60-80`).
    pub async fn list_versions(
        &self,
        limit: u32,
        offset: u32,
        locale: Option<&str>,
    ) -> Result<VersionList, UpdateError> {
        let query = vec![
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
            ("locale", locale.unwrap_or_default().to_owned()),
        ];
        let value = self.get("/versions", &query).await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `public.get_latest_version` (`public_api.py:82-92`): newest stable.
    pub async fn get_latest_version(
        &self,
        locale: Option<&str>,
    ) -> Result<VersionDetail, UpdateError> {
        let query = vec![("locale", locale.unwrap_or_default().to_owned())];
        let value = self.get("/versions/latest", &query).await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `public.get_latest_preview_version` (`public_api.py:94-104`):
    /// newest preview, `None` when none exists.
    pub async fn get_latest_preview_version(
        &self,
        locale: Option<&str>,
    ) -> Result<Option<VersionDetail>, UpdateError> {
        let query = vec![("locale", locale.unwrap_or_default().to_owned())];
        let value = self.get("/versions/latest-preview", &query).await?;
        Ok(serde_json::from_value(value)?)
    }
}

/// `VersionItem` as returned by the listing/latest endpoints.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionDetail {
    pub version: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub is_preview: bool,
    #[serde(default)]
    pub forced: bool,
}

/// `VersionListResponse` (`models.py:279-281`).
#[derive(Debug, Deserialize)]
pub struct VersionList {
    pub total: u32,
    #[serde(default)]
    pub data: Vec<VersionDetail>,
}

/// `detect_platform` (`_http.py:121-136`): the seven contract values;
/// this build runs on desktop OSes only, so the four mobile/web tags are
/// unreachable.
fn detect_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// `detect_platform_version` (`_http.py:139-179`): market version on
/// Windows/macOS, `distro version` from os-release on Linux; empty → no
/// header.
fn detect_platform_version() -> Option<String> {
    let raw = if cfg!(windows) {
        windows_product_version()
    } else if cfg!(target_os = "macos") {
        macos_marketing_version()
    } else {
        linux_release_version()
    };
    sanitize_platform_version(&raw)
}

/// Windows market version via RtlGetVersion (kernel 10.0 → 10/11 by build
/// number; 6.1-6.3 → 7/8/8.1, `_http.py WINDOWS_NT_TO_MARKET`).
#[cfg(windows)]
fn windows_product_version() -> String {
    use std::mem;
    use windows_sys::Win32::System::SystemInformation::{GetVersionExW, OSVERSIONINFOW};

    // ponytail: no manifest is embedded to force Win10 reporting, so
    // GetVersionExW can lie on compat-shimmed processes; RtlGetVersion via
    // GetVersionExW is the same source the Python build trusts. If servers
    // ever need exact builds, switch to RtlGetVersion (ntdll).
    let mut info: OSVERSIONINFOW = unsafe { mem::zeroed() };
    info.dwOSVersionInfoSize = mem::size_of::<OSVERSIONINFOW>() as u32;
    if unsafe { GetVersionExW(&mut info) } == 0 {
        return String::new();
    }
    match (info.dwMajorVersion, info.dwMinorVersion) {
        (10, 0) => (if info.dwBuildNumber >= 22000 {
            "11"
        } else {
            "10"
        })
        .to_owned(),
        (6, 1) => "7".to_owned(),
        (6, 2) => "8".to_owned(),
        (6, 3) => "8.1".to_owned(),
        _ => String::new(),
    }
}

#[cfg(not(windows))]
fn windows_product_version() -> String {
    String::new()
}

/// macOS marketing version (`_http.py _macos_marketing_version`):
/// `15.3.1` → `15`, `10.15.7` → `10.15`.
fn macos_marketing_version() -> String {
    // ponytail: no `sw_vers` output parsing — ProductVersion is read
    // straight from the plist the command reads itself.
    let Ok(plist) = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
    else {
        return String::new();
    };
    let Some(version) = extract_plist_value(&plist, "ProductVersion") else {
        return String::new();
    };
    let parts: Vec<&str> = version.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return String::new();
    }
    if parts[0] == "10" {
        parts.iter().take(2).cloned().collect::<Vec<_>>().join(".")
    } else {
        parts[0].to_owned()
    }
}

fn extract_plist_value(xml: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let start = xml.find(&marker)? + marker.len();
    let rest = &xml[start..];
    let value_start = rest.find("<string>")? + "<string>".len();
    let value_end = rest[value_start..].find("</string>")? + value_start;
    Some(rest[value_start..value_end].trim().to_owned())
}

/// Linux `ID VERSION_ID` from /etc/os-release
/// (`_http.py` freedesktop_os_release).
fn linux_release_version() -> String {
    let Ok(content) = std::fs::read_to_string("/etc/os-release") else {
        return String::new();
    };
    let mut id = String::new();
    let mut version = String::new();
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("ID=") {
            id = v.trim_matches('"').trim().to_lowercase();
        } else if let Some(v) = line.strip_prefix("VERSION_ID=") {
            version = v.trim_matches('"').trim().to_owned();
        }
    }
    format!("{id} {version}")
}

/// `sanitize_platform_version` (`_http.py:60-71`): printable ASCII only,
/// whitespace collapsed, capped at 32 chars; empty → no header.
fn sanitize_platform_version(value: &str) -> Option<String> {
    let ascii: String = value
        .chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { ' ' })
        .collect();
    let collapsed = ascii.split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = collapsed.chars().take(32).collect();
    let capped = capped.trim_end().to_owned();
    if capped.is_empty() {
        None
    } else {
        Some(capped)
    }
}

/// `format_client_time` (`_http.py:103-118`): local time, millisecond
/// precision, explicit `±HH:MM` offset — the shape the server requires.
///
/// No tz database dependency: the offset is taken from the system's local
/// time (`localtime_r` equivalent via `SystemTime` arithmetic is avoided —
/// libc gives the offset the OS applies, DST included).
fn format_client_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();

    let offset = local_utc_offset_secs(secs);
    let local = secs + offset;
    let (sign, off) = if offset < 0 {
        ('-', -offset)
    } else {
        ('+', offset)
    };

    let days = local.div_euclid(86_400);
    let time_of_day = local.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}{sign}{:02}:{:02}",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
        off / 3600,
        (off % 3600) / 60
    )
}

/// Local-vs-UTC offset at the given instant, from the OS timezone
/// (`localtime_r`), 0 when the OS has none set (SDK formats whatever the
/// host believes; UTC is the honest fallback).
#[cfg(not(windows))]
fn local_utc_offset_secs(unix_secs: i64) -> i64 {
    unsafe {
        let t: libc::time_t = unix_secs as _;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        // tm_gmtoff already accounts for DST at this instant.
        tm.tm_gmtoff as i64
    }
}

#[cfg(windows)]
fn local_utc_offset_secs(unix_secs: i64) -> i64 {
    unsafe {
        let t: libc::time_t = unix_secs as _;
        let mut local: libc::tm = std::mem::zeroed();
        let mut utc: libc::tm = std::mem::zeroed();
        // libc on Windows: localtime_s/gmtime_s (errno_t != 0 on failure),
        // no tm_gmtoff — offset = local-as-field vs utc-as-field, like the
        // C `tm_gmtoff` derivation.
        if libc::localtime_s(&mut local, &t) != 0 || libc::gmtime_s(&mut utc, &t) != 0 {
            return 0;
        }
        let (l, u) = (&local, &utc);
        let day = l.tm_yday as i64 * 86_400
            + l.tm_hour as i64 * 3_600
            + l.tm_min as i64 * 60
            + l.tm_sec as i64;
        let day_utc = u.tm_yday as i64 * 86_400
            + u.tm_hour as i64 * 3_600
            + u.tm_min as i64 * 60
            + u.tm_sec as i64;
        let mut diff = day - day_utc;
        // Wrap-around dates (local = Dec 31, UTC = Jan 1, and vice versa).
        if diff > 86_400 {
            diff -= 86_400 * 2 * ((diff + 86_400) / (86_400 * 2));
        } else if diff < -86_400 {
            diff += 86_400 * 2 * ((-diff + 86_400) / (86_400 * 2));
        }
        diff
    }
}

/// Days-since-epoch → civil date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Percent-encode one path/query component (`_resolve_path`:
/// `quote(value, safe="")`; `_build_url` uses `urlencode`).
fn urlencode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `_error_message` (`_http.py:573-587`): `message` may be a string or a
/// NestJS validation array.
fn error_message(body: &serde_json::Value) -> Option<String> {
    let message = body.get("message")?;
    match message {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|i| i.as_str())
                .map(str::to_owned)
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("; "))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_mapping_matches_python() {
        assert_eq!(to_verhub_locale(None), None);
        assert_eq!(to_verhub_locale(Some("")), None);
        assert_eq!(to_verhub_locale(Some("  ")), None);
        assert_eq!(to_verhub_locale(Some("Auto")), None);
        assert_eq!(to_verhub_locale(Some("auto")), None);
        assert_eq!(to_verhub_locale(Some("zh_CN")), Some("zh-CN"));
        assert_eq!(to_verhub_locale(Some("zh_HK")), Some("zh-TW"));
        assert_eq!(to_verhub_locale(Some("zh-Hant-TW")), Some("zh-TW"));
        assert_eq!(to_verhub_locale(Some("zh")), Some("zh-CN"));
        assert_eq!(to_verhub_locale(Some("en")), Some("en-US"));
        assert_eq!(to_verhub_locale(Some("fr_FR")), Some("en-US"));
    }

    #[test]
    fn parse_response_flattens_latest_version() {
        let value = serde_json::json!({
            "should_update": true, "required": false, "reason_codes": ["behind"],
            "latest_version": {"version": "2.22.0", "title": "修复", "content": "说明", "download_url": "https://x/y"}
        });
        let info = parse_response(&value);
        assert_eq!(info.version, "2.22.0");
        assert_eq!(info.title.as_deref(), Some("修复"));
        assert_eq!(info.content, "说明");
        assert_eq!(info.update_url, "https://x/y");
        assert!(info.should_update);
        assert!(!info.required);
    }

    #[test]
    fn parse_response_defaults_missing_fields() {
        let value = serde_json::json!({
            "should_update": false, "required": true,
            "latest_version": {"version": "2.22.0"}
        });
        let info = parse_response(&value);
        assert_eq!(info.content, "");
        assert_eq!(info.update_url, "");
        assert_eq!(info.title, None);
        assert!(info.required);
    }

    #[test]
    fn skip_gate_suppresses_only_that_version_and_only_automatic() {
        let info = UpdateInfo {
            should_update: true,
            required: false,
            version: "2.22.0".to_owned(),
            title: None,
            content: String::new(),
            update_url: String::new(),
        };
        assert!(suppresses_prompt(&info, "2.22.0", false));
        assert!(
            !suppresses_prompt(&info, "2.22.0", true),
            "manual check always prompts"
        );
        assert!(
            !suppresses_prompt(&info, "2.23.0", false),
            "other versions still prompt"
        );
        assert!(!suppresses_prompt(&info, "", false), "no skip stored");
    }

    #[test]
    fn skip_gate_never_suppresses_up_to_date() {
        let info = UpdateInfo {
            should_update: false,
            version: "2.20.0".to_owned(),
            ..upgrade_info()
        };
        assert!(!suppresses_prompt(&info, "2.20.0", false));
    }

    fn upgrade_info() -> UpdateInfo {
        UpdateInfo {
            should_update: true,
            required: false,
            version: String::new(),
            title: None,
            content: String::new(),
            update_url: String::new(),
        }
    }

    #[test]
    fn client_time_shape_matches_sdk_regex() {
        // SDK shape: ^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}[+-]\d{2}:\d{2}$
        let value = format_client_time();
        let b = value.as_bytes();
        let digit = |i: usize| b[i].is_ascii_digit();
        assert_eq!(b.len(), 29, "got {value}");
        let separators = [
            (4usize, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'.'),
            (23, b'+'), // or '-'
            (26, b':'),
        ];
        assert!(b[23] == b'+' || b[23] == b'-');
        for (i, expect) in separators.iter().filter(|(i, _)| *i != 23) {
            assert_eq!(b[*i], *expect, "byte {i} of {value}");
        }
        for i in [
            0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22, 24, 25, 27, 28,
        ] {
            assert!(digit(i), "byte {i} not a digit in {value}");
        }
    }

    #[test]
    fn platform_version_sanitized_to_32_printable_chars() {
        assert_eq!(sanitize_platform_version(""), None);
        assert_eq!(sanitize_platform_version("   "), None);
        assert_eq!(
            sanitize_platform_version("ubuntu 24.04"),
            Some("ubuntu 24.04".to_owned())
        );
        assert_eq!(
            sanitize_platform_version("a\nb\tc"),
            Some("a b c".to_owned())
        );
        let long = "x".repeat(40);
        assert_eq!(sanitize_platform_version(&long).map(|s| s.len()), Some(32));
    }

    #[test]
    fn urlencode_components() {
        assert_eq!(urlencode_component("a b/c"), "a%20b%2Fc");
        assert_eq!(
            urlencode_component("scottsloan-bili23-downloader"),
            "scottsloan-bili23-downloader"
        );
    }

    #[test]
    fn civil_date_conversion() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // 2024-01-01
    }

    #[test]
    fn macos_plist_extraction() {
        let plist = "<dict><key>ProductVersion</key><string>15.3.1</string></dict>";
        assert_eq!(
            extract_plist_value(plist, "ProductVersion").as_deref(),
            Some("15.3.1")
        );
    }
}
