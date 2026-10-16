//! Parse-history service: thin wrapper over `database::HistoryDatabase`
//! (cap 100, URL dedupe — covered by `crates/database` goldens), gated on
//! `Behavior/parse_history` like the Python settings card. The database
//! path is supplied by the caller so headless tests can point it at a
//! temp dir.

use config::ConfigStore;
use database::HistoryDatabase;
/// `Behavior/parse_history` (default true, `config.py:335`).
pub fn enabled(cfg: &ConfigStore) -> bool {
    cfg.get("Behavior", "parse_history").and_then(|v| v.as_bool()).unwrap_or(true)
}

/// Record a parsed URL if history is enabled. Dedupe + cap-100 behavior
/// live in `HistoryDatabase::add`.
pub fn record(
    db: &HistoryDatabase,
    cfg: &ConfigStore,
    title: &str,
    url: &str,
    type_: &str,
) -> anyhow::Result<()> {
    if !enabled(cfg) {
        return Ok(());
    }
    Ok(db.add(title, url, type_)?)
}
