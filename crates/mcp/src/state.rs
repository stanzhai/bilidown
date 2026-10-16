//! Shared server state backing the tools: config store, task manager,
//! parse list view and the one-parse-at-a-time guards.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use parsers::view::ParseListView;
use tasks::TaskManager;

/// Media-info summary captured after the last parse; feeds
/// `parse_url`/`get_episodes` output and gates `create_download`.
#[derive(Clone, Default)]
pub struct PreviewSummary {
    /// Resolved label entries, e.g. `("1080P", 80)`.
    pub video_quality: Vec<(String, i64)>,
    pub audio_quality: Vec<(String, i64)>,
    pub video_codec: Vec<(String, i64)>,
    pub title: String,
    pub number: String,
    pub from_fallback: bool,
    pub error: Option<String>,
}

/// Fingerprint of the parse-list snapshot, so auto-checked link targets
/// don't trip the "user has a selection" guard (Python `_last_snapshot`).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ParseSnapshot {
    pub all: HashSet<String>,
    pub checked: HashSet<String>,
}

/// Position ids created since startup; the Rust `TreeItem` has no
/// `downloaded` field, so the MCP layer tracks them itself.
pub type DownloadedSet = HashSet<String>;

pub struct McpState {
    pub config: Arc<Mutex<config::ConfigStore>>,
    pub manager: Arc<TaskManager>,
    pub view: Arc<Mutex<ParseListView>>,
    /// Side data for the parsed tree (Python's global `EpisodeData`);
    /// `manager.create` reads it keyed by `episode_id`.
    pub episode_data: Arc<parsers::EpisodeData>,
    /// Serialized with the parse lock, never behind `try_lock` long.
    pub preview: Mutex<Option<PreviewSummary>>,
    pub snapshot: Mutex<Option<ParseSnapshot>>,
    pub downloaded: Mutex<DownloadedSet>,
    /// Parse replaces the parse list: one at a time, 2s wait.
    pub parse_guard: Mutex<()>,
    /// Create serializes against itself only, 5s wait.
    pub create_guard: Mutex<()>,
    /// Wiremock base URL in tests, empty in production.
    pub api_base: String,
}

impl McpState {
    pub fn new(
        config: Arc<Mutex<config::ConfigStore>>,
        manager: Arc<TaskManager>,
        api_base: &str,
    ) -> Self {
        Self {
            config,
            manager,
            view: Arc::new(Mutex::new(ParseListView::new())),
            episode_data: Arc::new(parsers::EpisodeData::new()),
            preview: Mutex::new(None),
            snapshot: Mutex::new(None),
            downloaded: Mutex::new(HashSet::new()),
            parse_guard: Mutex::new(()),
            create_guard: Mutex::new(()),
            api_base: api_base.to_owned(),
        }
    }

    /// The `(all, checked)` id pair describing the current parse list.
    pub fn current_snapshot(&self, view: &MutexGuard<'_, ParseListView>) -> ParseSnapshot {
        let items = view.get_all_items();
        let mut all = HashSet::with_capacity(items.len());
        let mut checked = HashSet::new();
        for (index, item) in items.iter().enumerate() {
            let id = (index + 1).to_string();
            if item.checked == parsers::tree::CheckState::Checked {
                checked.insert(id.clone());
            }
            all.insert(id);
        }
        ParseSnapshot { all, checked }
    }

    /// True when the user's selection differs from the last parse — i.e.
    /// they actively picked something. Auto-checked link targets match the
    /// recorded snapshot and pass.
    pub fn user_has_selection(&self) -> bool {
        let view = self.view.lock();
        let current = self.current_snapshot(&view);
        let snapshot = self.snapshot.lock();
        let Some(last) = snapshot.as_ref() else {
            return !current.checked.is_empty();
        };
        current.checked != last.checked
    }
}
