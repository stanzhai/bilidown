//! 迁移链行为测试：与 Python test/test_config_migration.py 的场景一一对应。

use std::fs;

use config::{ConfigStore, APP_CONFIG_VERSION};
use serde_json::{json, Value};

fn base(version: i64) -> String {
    serde_json::json!({
        "Application": { "config_version": version },
    })
    .to_string()
}

fn write(dir: &std::path::Path, text: &str) {
    fs::write(dir.join("config.json"), text).unwrap();
}

/// v2120/v2160 输入里的分组合并由 golden 测试覆盖；这里用合成文件逐条验迁移门禁。
fn full(version: i64, extra: Value) -> String {
    let mut doc = serde_json::Map::new();
    doc.insert(
        "Application".into(),
        serde_json::json!({ "config_version": version }),
    );
    for (g, v) in extra.as_object().unwrap() {
        doc.insert(g.clone(), v.clone());
    }
    Value::Object(doc).to_string()
}

fn opened(dir: &std::path::Path) -> ConfigStore {
    // download_path 注入与金样一致；迁移门禁断言用不到它
    ConfigStore::with_paths(dir, "/home/test/Downloads")
}

// ---------- 2.13.0: proxy_enabled -> proxy_mode ----------

#[test]
fn proxy_enabled_true_becomes_manual() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2100,
            serde_json::json!({ "Advanced": { "proxy_enabled": true, "proxy_mode": "system" } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Advanced", "proxy_mode"),
        Some(serde_json::json!("manual"))
    );
}

#[test]
fn proxy_enabled_false_keeps_loaded_mode() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2100,
            serde_json::json!({ "Advanced": { "proxy_enabled": false, "proxy_mode": "system" } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Advanced", "proxy_mode"),
        Some(serde_json::json!("system"))
    );
}

#[test]
fn missing_proxy_enabled_is_safe() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), &base(2100));
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Advanced", "proxy_mode"),
        Some(serde_json::json!("system"))
    );
}

#[test]
fn version_2130_skips_proxy_gate() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2130,
            serde_json::json!({ "Advanced": { "proxy_enabled": true, "proxy_mode": "system" } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Advanced", "proxy_mode"),
        Some(serde_json::json!("system"))
    );
}

// ---------- 2.14.0: video_quality_priority 补 122 ----------

#[test]
fn quality_122_inserted_before_120() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2130,
            serde_json::json!({ "Download": { "video_quality_priority": [127, 126, 125, 120, 116, 80] } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Download", "video_quality_priority"),
        Some(serde_json::json!([127, 126, 125, 122, 120, 116, 80]))
    );
}

#[test]
fn quality_122_appended_when_no_120() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2130,
            serde_json::json!({ "Download": { "video_quality_priority": [116, 80] } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Download", "video_quality_priority"),
        Some(serde_json::json!([116, 80, 122]))
    );
}

#[test]
fn existing_122_not_duplicated() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2130,
            serde_json::json!({ "Download": { "video_quality_priority": [127, 122, 120] } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Download", "video_quality_priority"),
        Some(serde_json::json!([127, 122, 120]))
    );
}

#[test]
fn user_quality_order_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2130,
            serde_json::json!({ "Download": { "video_quality_priority": [80, 120, 127] } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Download", "video_quality_priority"),
        Some(serde_json::json!([80, 122, 120, 127]))
    );
}

#[test]
fn version_2140_skips_quality_gate() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2140,
            serde_json::json!({ "Download": { "video_quality_priority": [127, 120] } }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Download", "video_quality_priority"),
        Some(serde_json::json!([127, 120]))
    );
}

// ---------- 2.20.0: naming_rule_list 重置 + ov_cdn_server_list 去 Akamai ----------

#[test]
fn naming_rule_list_reset_to_defaults_for_any_below_2200() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2199,
            serde_json::json!({ "File Naming": { "naming_rule_list": [{"type": 90, "name": "custom", "template": "x"}] } }),
        ),
    );
    let store = opened(tmp.path());
    let list = store.get("File Naming", "naming_rule_list").unwrap();
    let arr = list.as_array().unwrap();
    assert!(arr.len() >= 10, "默认命名规则表被恢复");
    assert!(arr
        .iter()
        .all(|r| r.get("name").and_then(Value::as_str) != Some("custom")));
    assert!(store.naming_rules_reset());
}

#[test]
fn naming_rules_reset_false_at_2200() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), &base(APP_CONFIG_VERSION));
    let store = opened(tmp.path());
    assert!(!store.naming_rules_reset());
}

#[test]
fn version_2000_runs_whole_chain() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &full(
            2000,
            serde_json::json!({
                "Advanced": { "proxy_enabled": true },
                "Download": { "video_quality_priority": [127, 120] }
            }),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        store.get("Advanced", "proxy_mode"),
        Some(serde_json::json!("manual"))
    );
    assert_eq!(
        store.get("Download", "video_quality_priority"),
        Some(serde_json::json!([127, 122, 120]))
    );
    assert_eq!(
        store.get("Application", "config_version"),
        Some(serde_json::json!(APP_CONFIG_VERSION))
    );
    assert!(store.naming_rules_reset());
}

#[test]
fn written_version_is_2200() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), &base(2130));
    opened(tmp.path());
    let saved = fs::read_to_string(tmp.path().join("config.json")).unwrap();
    assert!(saved.contains("\"config_version\": 2200"));
}

// ---------- ov_cdn_server_list ----------

fn cdn_doc(version: i64, list: Value) -> String {
    full(
        version,
        serde_json::json!({ "Advanced": { "ov_cdn_server_list": list } }),
    )
}

fn cdn_list(store: &ConfigStore) -> Vec<String> {
    store
        .get("Advanced", "ov_cdn_server_list")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            e.get("host")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

#[test]
fn akamai_removed_rest_kept() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &cdn_doc(
            2160,
            serde_json::json!([
                { "provider": "AKAMAI", "host": "upos-hz-mirrorakam.akamaized.net" },
                { "provider": "ALIYUN", "host": "upos-sz-mirrorcoso1.bilivideo.com" }
            ]),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(cdn_list(&store), ["upos-sz-mirrorcoso1.bilivideo.com"]);
}

#[test]
fn user_added_nodes_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &cdn_doc(
            2160,
            serde_json::json!([
                { "provider": "ME", "host": "my.example.com" },
                { "provider": "ALIYUN", "host": "upos-sz-mirrorcoso1.bilivideo.com" }
            ]),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(
        cdn_list(&store),
        ["my.example.com", "upos-sz-mirrorcoso1.bilivideo.com"]
    );
}

#[test]
fn already_clean_list_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let clean =
        serde_json::json!([{ "provider": "ALIYUN", "host": "upos-sz-mirrorcoso1.bilivideo.com" }]);
    write(tmp.path(), &cdn_doc(2160, clean));
    let store = opened(tmp.path());
    assert_eq!(cdn_list(&store), ["upos-sz-mirrorcoso1.bilivideo.com"]);
}

#[test]
fn version_2200_skips_cdn_gate() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &cdn_doc(
            APP_CONFIG_VERSION,
            serde_json::json!([{ "provider": "AKAMAI", "host": "upos-hz-mirrorakam.akamaized.net" }]),
        ),
    );
    let store = opened(tmp.path());
    assert_eq!(cdn_list(&store), ["upos-hz-mirrorakam.akamaized.net"]);
}

#[test]
fn malformed_entries_survive() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &cdn_doc(
            2160,
            serde_json::json!([
                { "provider": "AKAMAI", "host": "upos-hz-mirrorakam.akamaized.net" },
                {},
                { "provider": "ALIYUN" }
            ]),
        ),
    );
    let store = opened(tmp.path());
    let list = store
        .get("Advanced", "ov_cdn_server_list")
        .unwrap()
        .as_array()
        .unwrap()
        .len();
    assert_eq!(list, 2, "缺 host 的畸形项在过滤后原样保留");
}

#[test]
fn empty_cdn_list_is_safe() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), &cdn_doc(2160, serde_json::json!([])));
    let store = opened(tmp.path());
    assert!(store
        .get("Advanced", "ov_cdn_server_list")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn defaults_contain_no_akamai() {
    let tmp = tempfile::tempdir().unwrap();
    let store = opened(tmp.path());
    assert!(!cdn_list(&store)
        .iter()
        .any(|h| h == "upos-hz-mirrorakam.akamaized.net"));
}
