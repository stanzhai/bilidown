//! Parse-history service test: config gate over HistoryDatabase
//! (cap/dedupe behavior itself is covered by `crates/database` goldens).

use config::ConfigStore;
use database::HistoryDatabase;
use services::history::{enabled, record};

const BV: &str = "https://www.bilibili.com/video/BV1xx411c7mD";

#[test]
fn record_gated_on_behavior_flag() {
    let dir = tempfile::tempdir().unwrap();
    let db = HistoryDatabase::open(dir.path().join("history.db")).unwrap();
    let cfg = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert!(enabled(&cfg)); // Behavior/parse_history default true (config.py:335)

    // gate on → recorded
    record(&db, &cfg, "Title", BV, "normal").unwrap();
    assert_eq!(db.count().unwrap(), 1);

    // re-recording the same URL dedupes, title updates (database behavior,
    // verified through the service path)
    record(&db, &cfg, "Title v2", BV, "normal").unwrap();
    assert_eq!(db.count().unwrap(), 1);
    assert_eq!(db.query_all().unwrap()[0].title, "Title v2");

    // gate off → nothing more recorded
    let mut cfg = cfg;
    cfg.set("Behavior", "parse_history", serde_json::Value::Bool(false));
    record(&db, &cfg, "Another", "https://www.bilibili.com/bangumi/play/ep123", "bangumi")
        .unwrap();
    assert_eq!(db.count().unwrap(), 1);
}
