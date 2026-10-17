//! CDN host selection: area lists, host replacement, and cross-task health
//! tracking (ticket 14).
//!
//! Ports the observable behavior of Python `util/network/cdn.py` +
//! `area.py`:
//! - [`parse_area`] maps a `x/web-interface/zone` response to
//!   [`Area`] (CN groups 86/852/853/886 — the split follows measured CDN
//!   reachability, not administrative borders: a HK exit reads the CN
//!   mirrors *faster* than the OV ones).
//! - [`CdnConfig::default_server_list`] mirrors `DefaultValue.cn/ov_cdn_server_list`.
//! - [`candidate_urls`] flattens the ordered candidate list: preferred
//!   layer first, the other layer as fallback, deduplicated, with hosts
//!   in cooldown sunk to the tail **within their own layer only**.
//! - [`HostHealth`] is process-global shared state under a lock, exactly
//!   like the Python class state: batch downloads run many resolves at
//!   once and must not each re-discover the same dead node.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use reqwest::Url;

/// Country codes grouped into [`Area::CN`] (`CHINA_COUNTRY_CODES`).
/// 港澳台（852/853/886）按实测 CDN 可达性归入 CN，而不是按行政区划。
pub const CHINA_COUNTRY_CODES: [i64; 4] = [86, 852, 853, 886];

/// Zone endpoint used for area detection (`AREA_ZONE_API`).
pub const AREA_ZONE_API: &str = "https://api.bilibili.com/x/web-interface/zone";

/// CDN exit area; selects which server list replaces hosts (`Area` enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Cn,
    Ov,
}

/// Which layer a replaced link came from (`prefer_cdn_server_provider`).
/// True (default) = provider mirrors first, original dispatch links
/// behind; false = the reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreferProvider(pub bool);

/// One CDN mirror entry (`cn/ov_cdn_server_list` item).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdnServer {
    pub host: String,
    pub provider: String,
}

impl CdnServer {
    pub fn new(host: &str, provider: &str) -> Self {
        Self {
            host: host.to_owned(),
            provider: provider.to_owned(),
        }
    }
}

/// Default mirror lists per area; deep-owned so callers may edit freely.
#[derive(Debug, Clone, Default)]
pub struct CdnConfig {
    pub area: Option<Area>,
    pub prefer_provider: PreferProvider,
    pub cn_servers: Vec<CdnServer>,
    pub ov_servers: Vec<CdnServer>,
}

impl CdnConfig {
    /// `DefaultValue.cn_cdn_server_list` verbatim.
    pub fn default_cn_servers() -> Vec<CdnServer> {
        [
            ("upos-sz-mirror08c.bilivideo.com", "HUAWEI"),
            ("upos-sz-mirrorhw.bilivideo.com", "HUAWEI"),
            ("upos-sz-mirrorhwb.bilivideo.com", "HUAWEI"),
            ("upos-sz-mirrorcos.bilivideo.com", "TENCENT"),
            ("upos-sz-mirrorcosb.bilivideo.com", "TENCENT"),
            ("upos-sz-mirrorcoso1.bilivideo.com", "TENCENT"),
            ("upos-sz-mirrorali.bilivideo.com", "ALIYUN"),
            ("upos-sz-mirroralib.bilivideo.com", "ALIYUN"),
        ]
        .into_iter()
        .map(|(host, provider)| CdnServer::new(host, provider))
        .collect()
    }

    /// `DefaultValue.ov_cdn_server_list` verbatim. Akamai is absent on
    /// purpose: rewritten links to it 403 regardless of exit area, so it
    /// only wastes a probe slot (see config.py note).
    pub fn default_ov_servers() -> Vec<CdnServer> {
        [
            ("upos-sz-mirroraliov.bilivideo.com", "ALIYUN"),
            ("upos-sz-mirrorcosov.bilivideo.com", "TENCENT"),
        ]
        .into_iter()
        .map(|(host, provider)| CdnServer::new(host, provider))
        .collect()
    }

    /// List used for host replacement in the configured area (None area
    /// = no replacement, mirroring an unset `config.area`).
    pub fn server_list(&self) -> &[CdnServer] {
        match self.area {
            Some(Area::Cn) => &self.cn_servers,
            Some(Area::Ov) => &self.ov_servers,
            None => &[],
        }
    }

    /// Ordered candidate list from playurl links (`CDN.get_candidate_urls`).
    pub fn candidate_urls(&self, url_list: &[String]) -> Vec<String> {
        candidate_urls(url_list, self.server_list(), self.prefer_provider)
    }
}

/// Consecutive probe failures before a host enters cooldown
/// (`HOST_FAILURE_THRESHOLD`).
pub const HOST_FAILURE_THRESHOLD: u32 = 2;

/// How long a failed host stays demoted (`HOST_COOLDOWN`).
pub const HOST_COOLDOWN: Duration = Duration::from_secs(180);

/// Consecutive probe failures that demote a host for [`HOST_COOLDOWN`]
/// (`HostHealth`). Shared across tasks: batch downloads spawn many
/// resolves, and without shared state each one re-discovers the same dead
/// node and pays a full timeout round again. Demoted, never dropped: the
/// failure may be a local outage, and ejecting nodes can empty the list.
#[derive(Default)]
pub struct HostHealth {
    inner: Mutex<HostHealthInner>,
}

#[derive(Default)]
struct HostHealthInner {
    failures: HashMap<String, u32>,
    cooldown_until: HashMap<String, Instant>,
}

static GLOBAL_HOST_HEALTH: LazyLock<HostHealth> = LazyLock::new(HostHealth::default);

fn host_health() -> &'static HostHealth {
    &GLOBAL_HOST_HEALTH
}

impl HostHealth {
    /// A probe succeeded: clear the host's failure count and cooldown.
    pub fn report_success(&self, host: &str) {
        if host.is_empty() {
            return;
        }
        let mut state = self.inner.lock();
        state.failures.remove(host);
        state.cooldown_until.remove(host);
    }

    /// A probe failed. At [`HOST_FAILURE_THRESHOLD`] consecutive failures
    /// the host is demoted for [`HOST_COOLDOWN`].
    pub fn report_failure(&self, host: &str) {
        if host.is_empty() {
            return;
        }
        let mut state = self.inner.lock();
        let count = state.failures.entry(host.to_owned()).or_insert(0);
        *count += 1;
        if *count >= HOST_FAILURE_THRESHOLD {
            state
                .cooldown_until
                .insert(host.to_owned(), Instant::now() + HOST_COOLDOWN);
        }
    }

    /// True while the host is demoted. An expired cooldown resets the
    /// failure count, giving the host a fresh chance.
    pub fn is_cooling(&self, host: &str) -> bool {
        if host.is_empty() {
            return false;
        }
        let mut state = self.inner.lock();
        let Some(&until) = state.cooldown_until.get(host) else {
            return false;
        };
        if Instant::now() >= until {
            state.cooldown_until.remove(host);
            state.failures.remove(host);
            return false;
        }
        true
    }
}

/// `HostHealth.report_success(host)` on the process-global tracker.
pub fn report_host_success(host: &str) {
    host_health().report_success(host);
}

/// `HostHealth.report_failure(host)` on the process-global tracker.
pub fn report_host_failure(host: &str) {
    host_health().report_failure(host);
}

/// `HostHealth.is_cooling(host)` on the process-global tracker.
pub fn host_is_cooling(host: &str) -> bool {
    host_health().is_cooling(host)
}

/// Test hook: clear the process-global health state (the pytest
/// `clean_host_health` autouse fixture, needed because cooldown is 180 s
/// and would otherwise leak across tests).
pub fn clear_host_health_for_test() {
    let mut state = host_health().inner.lock();
    state.failures.clear();
    state.cooldown_until.clear();
}

/// Host of a URL, or `""` when unparseable (`CDN.get_netloc`; malformed
/// IPv6 literals etc. behave as no-host rather than panicking).
pub fn netloc(url: &str) -> String {
    Url::parse(url)
        .map(|u| u.host_str().map_or_else(String::new, |h| h.to_owned()))
        .unwrap_or_default()
}

/// Deduplicate preserving first occurrence, sinking cooldown hosts to the
/// tail (`CDN.arrange`). `exclude` drops urls already present in the
/// other layer.
fn arrange(url_list: &[String], exclude: &std::collections::HashSet<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut healthy = Vec::new();
    let mut cooling = Vec::new();

    for url in url_list {
        if url.is_empty() || !seen.insert(url.clone()) || exclude.contains(url) {
            continue;
        }
        if host_is_cooling(&netloc(url)) {
            cooling.push(url.clone());
        } else {
            healthy.push(url.clone());
        }
    }

    healthy.extend(cooling);
    healthy
}

/// PCDN / P2P hosts rejected before probing (`CDN.filter` blacklist).
/// Matched against the host only: query strings can carry the same
/// substrings and must not false-positive.
const HOST_BLACKLIST: [&str; 7] = [
    "mcdn",
    "pcdn",
    "szbdyd.com",
    "mountaintoys.cn",
    // mirror14b answers 404 to HEAD and drops direct GETs; probing it can
    // only conclude failure, so it never becomes a candidate.
    "upos-sz-mirror14b.bilivideo.com",
    // Preventive P2P names from bilibili-accelerator's measured list.
    "nexusedgeio.com",
    "ahdohpiechei.com",
];

fn filter(url_list: &[String]) -> Vec<String> {
    url_list
        .iter()
        .filter(|url| {
            !url.is_empty() && {
                let host = netloc(url);
                !HOST_BLACKLIST.iter().any(|domain| host.contains(domain))
            }
        })
        .cloned()
        .collect()
}

/// Mall-course videos live in a separate bucket most mirrors lack, so
/// their links skip host replacement entirely (`CDN.is_mall_course_url`).
fn is_mall_course_url(url: &str) -> bool {
    url.contains("/mallxcodeboss/") || url.contains("gen=playurlv2_itemsvideo")
}

/// Rewrite the URL's host (`CDN.replace_netloc`); same host returns the
/// URL unchanged so dedup keeps one copy.
fn replace_netloc(url: &str, new_netloc: &str) -> Option<String> {
    let mut parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    // `new_netloc` may carry a port ("host:port"), like the Python
    // netloc swap; without it the rewritten URL would fall back to the
    // scheme default port and miss the mirror.
    let (new_host, new_port) = match new_netloc.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()),
        None => (new_netloc, None),
    };
    if host == new_host && parsed.port() == new_port {
        return Some(url.to_owned());
    }
    parsed.set_host(Some(new_host)).ok()?;
    let port = new_port.or_else(|| parsed.port());
    let _ = parsed.set_port(port);
    Some(parsed.to_string())
}

/// Every candidate link rewritten to every mirror (`CDN.replace`), same
/// iteration order: links outer, nodes inner.
fn replace(url_list: &[String], servers: &[CdnServer]) -> Vec<String> {
    let mut out = Vec::new();
    for url in url_list {
        if is_mall_course_url(url) {
            continue;
        }
        for server in servers {
            if server.host.is_empty() {
                continue;
            }
            if let Some(new_url) = replace_netloc(url, &server.host) {
                out.push(new_url);
            }
        }
    }
    out
}

/// Ordered candidate list (`CDN.get_candidate_urls`): filter PCDN, swap
/// in mirrors, then preferred layer first and the other layer behind.
/// Empty-after-filter input falls back to the raw non-empty links — a
/// poor link beats no link. The two layers are arranged separately:
/// arranging them jointly would let a cooling preferred-layer link fall
/// behind healthy fallback links, silently overriding the preference.
pub fn candidate_urls(
    url_list: &[String],
    servers: &[CdnServer],
    prefer_provider: PreferProvider,
) -> Vec<String> {
    let mut filtered = filter(url_list);
    if filtered.is_empty() {
        filtered = url_list
            .iter()
            .filter(|url| !url.is_empty())
            .cloned()
            .collect();
    }

    let replaced = replace(&filtered, servers);

    let (primary, fallback) = if prefer_provider.0 {
        (&replaced, &filtered)
    } else {
        (&filtered, &replaced)
    };

    let mut primary = arrange(primary, &Default::default());
    let primary_set: std::collections::HashSet<String> = primary.iter().cloned().collect();
    primary.extend(arrange(fallback, &primary_set));
    primary
}

/// Parse a zone response into [`Area`] (`parse_area`). Any unparsable
/// shape returns None: the input is a network response where missing or
/// mistyped fields are normal; the caller decides the fallback.
pub fn parse_area(data: &serde_json::Value) -> Option<Area> {
    let zone = data.get("data")?;
    if data.get("code")?.as_i64() != Some(0) || !zone.is_object() {
        return None;
    }
    let country_code = zone.get("country_code")?;
    // JSON booleans are not integers; a bare `true` must not pass for 1.
    let code = country_code.as_i64()?;
    if country_code.is_boolean() || !country_code.is_i64() {
        return None;
    }
    Some(if CHINA_COUNTRY_CODES.contains(&code) {
        Area::Cn
    } else {
        Area::Ov
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODES: [&str; 3] = [
        "node-a.example.com",
        "node-b.example.com",
        "node-c.example.com",
    ];

    fn servers() -> Vec<CdnServer> {
        NODES.iter().map(|h| CdnServer::new(h, "TEST")).collect()
    }

    fn urls(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_owned()).collect()
    }

    fn hosts_of(list: &[String]) -> Vec<String> {
        list.iter().map(|u| netloc(u)).collect()
    }

    /// The pytest autouse fixture, plus a mutex held for the test body:
    /// tests share one process-global map and run on parallel threads,
    /// so a sibling's `clean()` between our failure reports would wipe
    /// our cooldown.
    fn clean() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        // Lock BEFORE clearing: a sibling's clear while we hold the lock
        // would wipe our own cooldown mid-body.
        let guard = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_host_health_for_test();
        guard
    }

    #[test]
    fn primary_layer_comes_before_fallback() {
        let _guard = clean();
        let originals = urls(&[
            "https://upos-origin.example.net/video.m4s",
            "https://upos-backup.example.net/audio.m4s",
        ]);
        let candidates = candidate_urls(&originals, &servers(), PreferProvider(true));

        let mut expected: Vec<String> = Vec::new();
        for _ in &originals {
            expected.extend(NODES.iter().map(|n| (*n).to_owned()));
        }
        expected.extend(hosts_of(&originals));
        assert_eq!(hosts_of(&candidates), expected);
    }

    #[test]
    fn prefer_false_puts_original_links_first() {
        let _guard = clean();
        let originals = urls(&["https://upos-origin.example.net/video.m4s"]);
        let candidates = candidate_urls(&originals, &servers(), PreferProvider(false));

        let mut expected = hosts_of(&originals);
        expected.extend(NODES.iter().map(|n| (*n).to_owned()));
        assert_eq!(hosts_of(&candidates), expected);
    }

    #[test]
    fn cooling_node_only_sinks_within_its_own_layer() {
        let _guard = clean();
        // Own nodes: tests run in parallel threads on one global map, so
        // a sibling's report_host_success must not clear this cooldown.
        let nodes = [
            "cool-a.example.com",
            "node-b.example.com",
            "node-c.example.com",
        ];
        let servers: Vec<CdnServer> = nodes.iter().map(|h| CdnServer::new(h, "TEST")).collect();
        let originals = urls(&["https://upos-origin.example.net/video.m4s"]);
        for _ in 0..HOST_FAILURE_THRESHOLD {
            report_host_failure(nodes[0]);
        }

        let candidates = candidate_urls(&originals, &servers, PreferProvider(true));

        // Cooled node sinks to the primary-layer tail but never behind
        // the fallback layer: layering carries the preference.
        let mut expected: Vec<String> = nodes[1..].iter().map(|n| (*n).to_owned()).collect();
        expected.push(nodes[0].to_owned());
        expected.extend(hosts_of(&originals));
        assert_eq!(hosts_of(&candidates), expected);
    }

    #[test]
    fn success_clears_cooldown() {
        let _guard = clean();
        report_host_failure(NODES[0]);
        assert!(!host_is_cooling(NODES[0]));
        report_host_failure(NODES[0]);
        assert!(host_is_cooling(NODES[0]));
        report_host_success(NODES[0]);
        assert!(!host_is_cooling(NODES[0]));
    }

    #[test]
    fn same_host_in_both_layers_is_kept_once() {
        let _guard = clean();
        let original = format!("https://{}/video.m4s", NODES[0]);
        let original_refs = [original.clone()];
        let candidates = candidate_urls(&original_refs, &servers(), PreferProvider(true));

        assert_eq!(candidates.iter().filter(|u| **u == original).count(), 1);
        assert_eq!(candidates.len(), NODES.len());
    }

    #[test]
    fn backup_links_of_one_resource_collapse_into_one_per_node() {
        let _guard = clean();
        let originals = urls(&[
            "https://upos-origin.example.net/upgcxcode/video.m4s?upsig=abc",
            "https://upos-backup.example.net/upgcxcode/video.m4s?upsig=abc",
        ]);
        let candidates = candidate_urls(&originals, &servers(), PreferProvider(true));

        let mut expected = NODES.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        expected.extend(hosts_of(&originals));
        assert_eq!(hosts_of(&candidates), expected);
        assert_eq!(candidates.len(), NODES.len() + 2);
    }

    #[test]
    fn mall_course_links_skip_node_replacement() {
        let _guard = clean();
        let original = "https://upos-origin.example.net/mallxcodeboss/video.m4s".to_owned();
        let original_refs = [original.clone()];
        assert_eq!(
            candidate_urls(&original_refs, &servers(), PreferProvider(true)),
            vec![original]
        );
    }

    #[test]
    fn all_filtered_links_fall_back_to_raw_list() {
        let _guard = clean();
        let pcdn = "https://x.pcdn.example.net/video.m4s".to_owned();
        let pcdn_refs = [pcdn.clone()];
        let candidates = candidate_urls(&pcdn_refs, &servers(), PreferProvider(true));

        assert!(candidates.contains(&pcdn));
        let mut expected = NODES.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        expected.push(netloc(&pcdn));
        assert_eq!(hosts_of(&candidates), expected);
    }

    #[test]
    fn pcdn_hosts_are_filtered_but_query_strings_are_not() {
        let _guard = clean();
        let list = urls(&[
            "https://upos-origin.example.net/video.m4s?from=pcdn",
            "https://x.mcdn.example.net/video.m4s",
        ]);
        let candidates = candidate_urls(&list, &[], PreferProvider(true));
        assert_eq!(hosts_of(&candidates), vec!["upos-origin.example.net"]);
    }

    #[test]
    fn empty_input_yields_no_candidates() {
        let _guard = clean();
        assert!(candidate_urls(&[], &servers(), PreferProvider(true)).is_empty());
    }

    #[test]
    fn no_servers_yields_originals_only() {
        let _guard = clean();
        let originals = urls(&["https://upos-origin.example.net/video.m4s"]);
        let candidates = candidate_urls(&originals, &[], PreferProvider(true));
        assert_eq!(candidates, originals);
    }

    #[test]
    fn replace_netloc_preserves_port_query_and_fragment() {
        let url = "https://upos-origin.example.net:8443/video.m4s?e=ot5ot5&upsig=abc#frag";
        let replaced = replace_netloc(url, NODES[0]).unwrap();
        assert_eq!(netloc(&replaced), NODES[0]);
        assert!(replaced.contains(":8443/video.m4s?e=ot5ot5&upsig=abc"));
        assert!(replaced.ends_with("#frag"));
        // Same host is returned unchanged so both layers dedup to one.
        assert_eq!(replace_netloc(url, "upos-origin.example.net").unwrap(), url);
    }

    fn zone_response(country_code: Option<serde_json::Value>) -> serde_json::Value {
        let mut data = serde_json::json!({
            "addr": "221.7.112.22",
            "country": "中国",
            "province": "重庆",
            "isp": "联通",
        });
        if let Some(code) = country_code {
            data["country_code"] = code;
        }
        serde_json::json!({ "code": 0, "message": "OK", "data": data })
    }

    #[test]
    fn mainland_is_cn() {
        assert_eq!(parse_area(&zone_response(Some(86.into()))), Some(Area::Cn));
    }

    #[test]
    fn overseas_is_ov() {
        for code in [1, 44, 81] {
            assert_eq!(
                parse_area(&zone_response(Some(code.into()))),
                Some(Area::Ov),
                "code {code}"
            );
        }
    }

    #[test]
    fn hong_kong_macau_taiwan_use_mainland_list() {
        for code in [852, 853, 886] {
            assert_eq!(
                parse_area(&zone_response(Some(code.into()))),
                Some(Area::Cn),
                "code {code}"
            );
        }
    }

    #[test]
    fn real_mainland_response_is_cn() {
        // Captured direct exit (重庆联通).
        let body = serde_json::json!({
            "code": 0, "message": "OK", "ttl": 1,
            "data": {
                "addr": "221.7.112.22", "country": "中国", "province": "重庆",
                "isp": "联通", "country_code": 86,
            },
        });
        assert_eq!(parse_area(&body), Some(Area::Cn));
    }

    #[test]
    fn real_hong_kong_response_is_cn() {
        // Captured Clash HK exit: Bilibili reports 852, explicitly grouped
        // into CN by measured mirror reachability.
        let body = serde_json::json!({
            "code": 0, "message": "OK", "ttl": 1,
            "data": {
                "addr": "157.254.20.4", "country": "香港",
                "latitude": 22.396428, "longitude": 114.109497,
                "zone_id": 1035993088, "country_code": 852,
            },
        });
        assert_eq!(parse_area(&body), Some(Area::Cn));
    }

    #[test]
    fn non_zero_code_is_none() {
        let body = serde_json::json!({
            "code": -352, "message": "风控", "data": {"country_code": 86}
        });
        assert_eq!(parse_area(&body), None);
    }

    #[test]
    fn malformed_shapes_are_none() {
        let missing_code = zone_response(None);
        assert_eq!(parse_area(&missing_code), None);
        assert_eq!(
            parse_area(&serde_json::json!({"code": 0, "message": "OK"})),
            None
        );
        assert_eq!(
            parse_area(&serde_json::json!({"code": 0, "message": "OK", "data": null})),
            None
        );
        assert_eq!(
            parse_area(&serde_json::json!({"code": 0, "data": "1.2.3.4"})),
            None
        );
        // No implicit conversions.
        for code in [
            serde_json::json!("86"),
            serde_json::json!(86.0),
            serde_json::json!([86]),
            serde_json::json!({"v": 86}),
        ] {
            assert_eq!(
                parse_area(&zone_response(Some(code.clone()))),
                None,
                "code {code:?}"
            );
        }
    }

    #[test]
    fn boolean_country_code_is_none() {
        for code in [true, false] {
            assert_eq!(parse_area(&zone_response(Some(code.into()))), None);
        }
    }

    #[test]
    fn non_object_input_is_none() {
        for body in [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!("ok"),
            serde_json::json!(0),
            serde_json::json!(1.5),
        ] {
            assert_eq!(parse_area(&body), None);
        }
    }

    #[test]
    fn default_server_lists_match_python_defaults() {
        let cn = CdnConfig::default_cn_servers();
        assert_eq!(cn.len(), 8);
        assert_eq!(cn[0].host, "upos-sz-mirror08c.bilivideo.com");
        assert_eq!(cn[0].provider, "HUAWEI");
        assert_eq!(cn[7].host, "upos-sz-mirroralib.bilivideo.com");

        let ov = CdnConfig::default_ov_servers();
        assert_eq!(ov.len(), 2);
        assert_eq!(ov[0].host, "upos-sz-mirroraliov.bilivideo.com");
        assert_eq!(ov[1].host, "upos-sz-mirrorcosov.bilivideo.com");
        // The Akamai entry was removed on purpose (403 on rewritten links).
        assert!(!ov.iter().any(|s| s.host.contains("akamai")));
    }

    #[test]
    fn config_selects_list_by_area() {
        let mut config = CdnConfig {
            cn_servers: CdnConfig::default_cn_servers(),
            ov_servers: CdnConfig::default_ov_servers(),
            ..Default::default()
        };
        config.area = Some(Area::Cn);
        assert_eq!(config.server_list().len(), 8);
        config.area = Some(Area::Ov);
        assert_eq!(config.server_list().len(), 2);
        config.area = None;
        assert!(config.server_list().is_empty());
    }
}
