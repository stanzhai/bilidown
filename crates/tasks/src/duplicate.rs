//! Duplicate detection (Python `_check_duplicate` / `is_duplicate` /
//! `_calc_hash_id`). The hash must equal what the parse side and the DB
//! side compute, so the raw episode dict is normalized here exactly like
//! Python's `_to_int`/`_to_str` kwargs and fed to the same branch order.

use crate::options::pick_option;
use crate::status::DuplicateResolution;
use config::ConfigStore;
use database::TaskDatabase;
use serde_json::Value;

fn to_int(value: Option<&Value>) -> i64 {
    match value {
        None | Some(Value::Null) => 0,
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Some(Value::String(s)) => s.parse().ok().filter(|_| !s.is_empty()).unwrap_or(0),
        Some(Value::Bool(b)) => *b as i64,
        _ => 0,
    }
}

fn to_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => {
            if s.is_empty() {
                String::new()
            } else {
                s.clone()
            }
        }
        Some(other) => other.to_string(),
    }
}

/// `calc_hash_id` from a raw episode dict (the parse-side entry point;
/// the DB side calls `database::calc_hash_id` on the built TaskInfo).
/// Branch order: VIDEO → BANGUMI → LESSON → CHEESE → AUDIO → task_id.
pub fn calc_hash_id_from_episode(episode: &serde_json::Map<String, Value>) -> String {
    // Normalize like Python's _to_int/_to_str kwargs, then reuse the one
    // DB-side hash implementation so parse side and storage side can
    // never drift. Branch order lives in database::calc_hash_id.
    let get = |k: &str| episode.get(k);
    let mut probe = database::TaskInfo::default();
    probe.Episode.attribute = to_int(get("attribute"));
    probe.Episode.aid = to_int(get("aid"));
    probe.Episode.bvid = to_str(get("bvid"));
    probe.Episode.cid = to_int(get("cid"));
    probe.Episode.ep_id = to_int(get("ep_id"));
    probe.Episode.sid = to_int(get("sid"));
    probe.Episode.course_id = to_int(get("course_id"));
    probe.Episode.lesson_id = to_int(get("lesson_id"));
    probe.Episode.item_id = to_int(get("item_id"));
    probe.Episode.section_id = to_int(get("section_id"));
    probe.Basic.task_id = to_str(get("task_id"));
    database::hash_id::calc_hash_id(&probe)
}

/// Answer for an ALWAYS_ASK duplicate: skip or continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskOutcome {
    Skip,
    Continue,
}

/// Outcome of the duplicate check: skip the episode or create the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateCheck {
    Skip,
    Continue,
}

/// `is_duplicate`: non-interactive pre-check, no dialog, no toast.
pub fn is_duplicate(db: &TaskDatabase, episode: &serde_json::Map<String, Value>) -> bool {
    // Python: a failed lookup returns False.
    db.check_duplicate(&calc_hash_id_from_episode(episode))
        .unwrap_or(false)
}

/// `_check_duplicate`. `ask` is the interactive resolver invoked only for
/// ALWAYS_ASK (the GUI dialog in Python; headless callers pick a side).
/// Returns `Skip` when the episode should not be downloaded.
pub fn check_duplicate(
    db: &TaskDatabase,
    config: &ConfigStore,
    episode: &serde_json::Map<String, Value>,
    options: Option<&serde_json::Map<String, Value>>,
    mut ask: impl FnMut() -> AskOutcome,
) -> DuplicateCheck {
    let hash_id = calc_hash_id_from_episode(episode);
    if !db.check_duplicate(&hash_id).unwrap_or(false) {
        return DuplicateCheck::Continue;
    }

    let fallback = config
        .get("Behavior", "duplicate_download_resolution")
        .unwrap_or_else(|| Value::from(2));
    let resolution = DuplicateResolution::from_code(
        pick_option(options, "duplicate_resolution", &fallback)
            .as_i64()
            .unwrap_or(2),
    );

    match resolution {
        DuplicateResolution::Continue => DuplicateCheck::Continue,
        DuplicateResolution::Skip => DuplicateCheck::Skip,
        DuplicateResolution::AlwaysAsk => match ask() {
            AskOutcome::Skip => DuplicateCheck::Skip,
            AskOutcome::Continue => DuplicateCheck::Continue,
        },
    }
}
