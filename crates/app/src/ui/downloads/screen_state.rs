//! Screen-level state the shell renders: active tab + the two lists +
//! the pending duplicate-ask queue entry.

use super::model::{SortKey, TaskListState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadsTab {
    Downloading,
    Completed,
}

impl DownloadsTab {
    /// `(sort_by_item, ascending_item)` config keys.
    pub fn sort_items(self) -> (&'static str, &'static str) {
        match self {
            Self::Downloading => (
                "downloading_list_sort_by",
                "downloading_list_sort_ascending",
            ),
            Self::Completed => ("completed_list_sort_by", "completed_list_sort_ascending"),
        }
    }
}

#[derive(Debug)]
pub struct ScreenState {
    pub tab: DownloadsTab,
    pub downloading: TaskListState,
    pub completed: TaskListState,
    /// Duplicate task awaiting the ask/skip/continue dialog
    /// (`_process_next_duplicate_download` queue head).
    pub pending_duplicate_ask: Option<String>,
    /// Set when the background refresh query failed (shell shows a hint).
    pub query_failed: bool,
    /// Decoded cover images keyed by cover URL (task rows render from
    /// this; fetched once per URL per session, `None` after a failure
    /// so broken covers don't refetch every frame).
    pub covers: std::collections::HashMap<String, Option<std::sync::Arc<gpui_kit::gpui::Image>>>,
    /// Cover URLs a fetch is already in flight for.
    pub covers_fetching: std::collections::HashSet<String>,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenState {
    pub fn new() -> Self {
        Self {
            tab: DownloadsTab::Downloading,
            downloading: TaskListState::new(false),
            completed: TaskListState::new(true),
            pending_duplicate_ask: None,
            query_failed: false,
            covers: Default::default(),
            covers_fetching: Default::default(),
        }
    }

    /// Read the persisted sort prefs (Behavior.<tab>_list_sort_*).
    pub fn load_sort_prefs(&mut self, cfg: &config::ConfigStore) {
        for (tab, list) in [
            (DownloadsTab::Downloading, &mut self.downloading),
            (DownloadsTab::Completed, &mut self.completed),
        ] {
            let (by, asc) = tab.sort_items();
            if let Some(key) = cfg
                .get("Behavior", by)
                .and_then(|v| v.as_str().map(str::to_owned))
                .and_then(|v| SortKey::from_config(&v))
            {
                list.sort_by = key;
            }
            if let Some(asc) = cfg.get("Behavior", asc).and_then(|v| v.as_bool()) {
                list.ascending = asc;
            } else if cfg.get("Behavior", asc).and_then(|v| v.as_i64()) == Some(1) {
                list.ascending = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_prefs_load_from_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config::ConfigStore::with_paths(dir.path(), dir.path());
        cfg.set(
            "Behavior",
            "downloading_list_sort_by",
            serde_json::json!("file_size"),
        );
        cfg.set(
            "Behavior",
            "downloading_list_sort_ascending",
            serde_json::json!(false),
        );
        cfg.set(
            "Behavior",
            "completed_list_sort_by",
            serde_json::json!("show_title"),
        );
        cfg.set(
            "Behavior",
            "completed_list_sort_ascending",
            serde_json::json!(true),
        );

        let mut s = ScreenState::new();
        s.load_sort_prefs(&cfg);
        assert_eq!(s.downloading.sort_by, SortKey::FileSize);
        assert!(!s.downloading.ascending);
        assert_eq!(s.completed.sort_by, SortKey::Title);
        assert!(s.completed.ascending);
    }

    #[test]
    fn defaults_when_config_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config::ConfigStore::with_paths(dir.path(), dir.path());
        let mut s = ScreenState::new();
        s.load_sort_prefs(&cfg);
        assert_eq!(s.downloading.sort_by, SortKey::CreatedTime);
        assert!(s.downloading.ascending);
        assert_eq!(s.completed.sort_by, SortKey::CompletedTime);
        assert!(s.completed.ascending);
    }
}
