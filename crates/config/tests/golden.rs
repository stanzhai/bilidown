//! 金样测试：Rust 写出的 config.json 必须与 Python 端真实实现产出的字节一致。
//!
//! 夹具由 Python 实现（config.py + qfluentwidgets 1.11.3）在隔离的
//! appdata 里生成，`*.expected.json` 即升级后的基准字节。

use std::fs;

use config::ConfigStore;

const DEFAULTS: &str = include_str!("fixtures/defaults.json");
const V2120: &str = include_str!("fixtures/v2120.json");
const V2120_EXPECTED: &str = include_str!("fixtures/v2120.expected.json");
const V2160: &str = include_str!("fixtures/v2160.json");
const V2160_EXPECTED: &str = include_str!("fixtures/v2160.expected.json");
const CORRUPT: &[u8] = include_bytes!("fixtures/corrupt.json");

/// 夹具里的 download_path 默认值（生成时把系统下载目录归一化成了它）。
const FIXTURE_DOWNLOAD_PATH: &str = "/home/test/Downloads";

#[test]
fn fresh_dir_creates_no_file_and_reports_no_reset() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);

    assert!(!store.path().exists(), "读取不得创建文件（Python 只告警）");
    assert!(!store.naming_rules_reset());
}

#[test]
fn save_writes_python_identical_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);
    store.set("Download", "download_path", serde_json::json!(FIXTURE_DOWNLOAD_PATH));

    store.save().unwrap();

    let bytes = fs::read(tmp.path().join("config.json")).unwrap();
    assert_eq!(bytes, DEFAULTS.as_bytes());
    // 原子写不留临时文件
    let entries: Vec<_> = fs::read_dir(tmp.path()).unwrap().collect();
    assert_eq!(entries.len(), 1, "目录里应只剩 config.json");
}

#[test]
fn old_python_config_upgrades_in_place_v2120() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("config.json"), V2120).unwrap();

    let store = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);

    let bytes = fs::read(tmp.path().join("config.json")).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        V2120_EXPECTED,
        "升级后的字节必须与 Python 迁移结果一致"
    );
    assert!(store.naming_rules_reset(), "命名规则被整表重置要置标志");

    // 迁移语义抽查（字节已覆盖，这里是可读的锚点）
    assert_eq!(store.get("Advanced", "proxy_mode"), Some(&serde_json::json!("manual")));
    assert_eq!(store.get("Download", "download_path"), Some(&serde_json::json!("/tmp/old-downloads")));
    let list: Vec<i64> = store.get("Download", "video_quality_priority").unwrap()
        .as_array().unwrap().iter().map(|q| q.as_i64().unwrap()).collect();
    let pos122 = list.iter().position(|q| *q == 122).unwrap();
    let pos120 = list.iter().position(|q| *q == 120).unwrap();
    assert_eq!(pos122 + 1, pos120, "122 要插到 120 之前");
}

#[test]
fn old_python_config_upgrades_in_place_v2160() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("config.json"), V2160).unwrap();

    let store = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);

    assert!(store.naming_rules_reset());
    // 2160 ≥ 2140：不补 122；代理三态已在盘上，不被 2.13.0 迁移改写
    assert_eq!(store.get("Advanced", "proxy_mode"), Some(&serde_json::json!("disabled")));
    assert_eq!(
        store.get("Download", "video_quality_priority"),
        Some(&serde_json::json!([127, 126, 80]))
    );
}

#[test]
fn corrupt_file_recovers_to_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("config.json"), CORRUPT).unwrap();

    let store = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);

    let bytes = fs::read(tmp.path().join("config.json")).unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), DEFAULTS);
    assert!(store.naming_rules_reset(), "版本未知按最旧处理，命名规则同样重置");
}

#[test]
fn current_config_is_loaded_not_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("config.json"), V2160_EXPECTED).unwrap();
    let before = fs::read(tmp.path().join("config.json")).unwrap();

    let store = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);

    assert_eq!(fs::read(tmp.path().join("config.json")).unwrap(), before, "已是最新的配置不得被改写");
    assert!(!store.naming_rules_reset());
    // 盘上的值要真的进了内存
    assert_eq!(store.get("Interface", "language"), Some(&serde_json::json!("en_US")));
    assert_eq!(store.get("Advanced", "area"), Some(&serde_json::json!("ov")));
}

#[test]
fn load_save_roundtrip_is_stable() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("config.json"), V2120).unwrap();
    ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);

    // 再开一次：已是 2200，不再迁移；再存一次字节不变
    let second = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);
    assert!(!second.naming_rules_reset());
    second.save().unwrap();
    assert_eq!(
        fs::read(tmp.path().join("config.json")).unwrap(),
        V2120_EXPECTED.as_bytes()
    );
}

#[test]
fn unknown_groups_and_items_are_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("config.json"), V2120).unwrap();
    let store = ConfigStore::with_paths(tmp.path(), FIXTURE_DOWNLOAD_PATH);

    let saved = fs::read_to_string(tmp.path().join("config.json")).unwrap();
    assert!(!saved.contains("LegacyGroup"));
    assert!(!saved.contains("app_name"));
    assert!(!saved.contains("sort_key"));
    assert!(!saved.contains("proxy_enabled"));
    assert!(!saved.contains("when_close\""));
    assert_eq!(store.get("Cookie", "SESSDATA"), Some(&serde_json::json!("")));
}

