//! Episode tree model for parse results (port of
//! `src/util/parse/episode/tree.py`).
//!
//! A parse produces a [`TreeItem`] root whose children (recursively) are the
//! downloadable Episodes. [`Attribute`] bitflags mark what each node is; the
//! global [`EpisodeData`] table carries the per-episode side data (uploader
//! info etc.) that a tree node references by `episode_id`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::RwLock;
use serde_json::{json, Map, Value};
use uuid::Uuid;

/// Node attribute bitflags (`Attribute` IntFlag). Values are wire-visible:
/// they land in the task JSON via `to_dict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attribute(pub u32);

impl Attribute {
    /// 投稿视频 (uploaded video).
    pub const VIDEO_BIT: Self = Self(1 << 0);
    /// 剧集 (bangumi).
    pub const BANGUMI_BIT: Self = Self(1 << 1);
    /// 课程 (cheese).
    pub const CHEESE_BIT: Self = Self(1 << 2);
    /// 每周必看 (weekly popular).
    pub const WEEKLY_BIT: Self = Self(1 << 3);
    /// 合集列表 (collection list).
    pub const COLLECTION_LIST_BIT: Self = Self(1 << 4);
    /// 个人空间 (uploader space).
    pub const SPACE_BIT: Self = Self(1 << 5);
    /// 收藏夹 (favorites folder).
    pub const FAVLIST_BIT: Self = Self(1 << 6);
    /// 需要二次解析 (needs re-parse: video inside a list).
    pub const NEED_PARSE_BIT: Self = Self(1 << 7);
    /// 单个视频 (single video item).
    pub const NORMAL_BIT: Self = Self(1 << 8);
    /// 分P (multi-P page item).
    pub const PART_BIT: Self = Self(1 << 9);
    /// 合集 (collection node).
    pub const COLLECTION_BIT: Self = Self(1 << 10);
    /// 互动视频 (interactive video item).
    pub const INTERACTIVE_BIT: Self = Self(1 << 11);
    /// 下载为单个视频 (download as a single video).
    pub const DOWNLOAD_AS_SINGLE_VIDEO_BIT: Self = Self(1 << 12);
    /// 稍后再看 (watch later).
    pub const WATCH_LATER_BIT: Self = Self(1 << 13);
    /// 历史记录 (history).
    pub const HISTORY_BIT: Self = Self(1 << 14);
    /// 树节点 (structural tree node, not downloadable itself).
    pub const TREE_NODE_BIT: Self = Self(1 << 15);
    /// 音频 (audio track).
    pub const AUDIO_BIT: Self = Self(1 << 16);
    /// 收藏夹中包含分P视频 (favorites folder containing multi-P videos).
    pub const FAVORITE_WITH_MULTI_PART_VIDEO_BIT: Self = Self(1 << 17);
    /// 会员购商城课程 (mall lesson).
    pub const LESSON_BIT: Self = Self(1 << 18);

    /// Bitwise union (`|`).
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when every bit of `self` is set.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Attribute {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self::union(self, rhs)
    }
}

/// Tri-state check mark of a tree node (`Qt.CheckState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckState {
    #[default]
    Unchecked,
    PartiallyChecked,
    Checked,
}

/// One node of the parse-result tree. Root nodes carry
/// [`Attribute::TREE_NODE_BIT`] plus a display `number` (the category
/// label); leaves are the Episodes.
#[derive(Debug, Clone)]
pub struct TreeItem {
    pub attribute: Attribute,
    pub checked: CheckState,

    pub pubtime: i64,
    pub favtime: i64,
    pub viewtime: i64,
    pub expired: bool,

    pub aid: i64,
    pub cid: i64,
    pub sid: i64,
    pub url: String,
    pub bvid: String,
    pub ep_id: i64,
    pub badge: String,
    pub cover: String,
    pub title: String,
    pub author: String,
    pub number: Number,
    pub duration: i64,
    pub episode_id: String,
    pub episode_plot: String,
    pub part_number: i64,
    pub episode_number: i64,
    pub related_titles: Map<String, Value>,

    pub uploader: String,
    pub uploader_uid: i64,

    pub course_id: i64,
    pub lesson_id: i64,
    pub item_id: i64,
    pub section_id: i64,

    pub children: Vec<TreeItem>,
}

/// The display `number` field: an episode index or the category label a
/// root node shows ("音频", "分P", ...).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Number {
    #[default]
    Empty,
    Int(i64),
    Text(String),
}

impl Number {
    pub fn to_json(&self) -> Value {
        match self {
            Number::Empty => Value::Null,
            Number::Int(n) => json!(n),
            Number::Text(s) => json!(s),
        }
    }
}

impl TreeItem {
    /// Build from the field set the Python `TreeItem(item_data)` reads;
    /// everything else defaults.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        number: Number,
        title: &str,
        cover: &str,
        duration: i64,
        sid: i64,
        pubtime: i64,
        url: &str,
        uploader: &str,
        related_titles: Map<String, Value>,
    ) -> Self {
        Self {
            attribute: Attribute::default(),
            checked: CheckState::default(),
            pubtime,
            favtime: 0,
            viewtime: 0,
            expired: false,
            aid: 0,
            cid: 0,
            sid,
            url: url.to_owned(),
            bvid: String::new(),
            ep_id: 0,
            badge: String::new(),
            cover: cover.to_owned(),
            title: title.to_owned(),
            author: String::new(),
            number,
            duration,
            episode_id: String::new(),
            episode_plot: String::new(),
            part_number: 0,
            episode_number: 0,
            related_titles,
            uploader: uploader.to_owned(),
            uploader_uid: 0,
            course_id: 0,
            lesson_id: 0,
            item_id: 0,
            section_id: 0,
            children: Vec::new(),
        }
    }

    /// Empty structural node.
    pub fn empty() -> Self {
        Self::new(Number::Empty, "", "", 0, 0, 0, "", "", Map::new())
    }

    /// Structural root node carrying a category label and title.
    pub fn root(number: Number, title: &str) -> Self {
        let mut node = Self::empty();
        node.number = number;
        node.title = title.to_owned();
        node
    }

    pub fn set_attribute(&mut self, flag: Attribute) {
        self.attribute = self.attribute | flag;
    }

    pub fn has_attribute(&self, flag: Attribute) -> bool {
        self.attribute.contains(flag)
    }

    pub fn add_child(&mut self, child: TreeItem) {
        self.children.push(child);
    }

    pub fn child(&self, row: usize) -> &TreeItem {
        &self.children[row]
    }

    pub fn count(&self) -> usize {
        self.children.len()
    }

    /// Recursive case-insensitive title search (`search_items`).
    pub fn search_items(&self, keyword: &str) -> Vec<&TreeItem> {
        let mut matches = Vec::new();
        if !keyword.is_empty() && self.title.to_lowercase().contains(&keyword.to_lowercase()) {
            matches.push(self);
        }
        for child in &self.children {
            matches.extend(child.search_items(keyword));
        }
        matches
    }

    /// Recursive leaves (`get_all_leaves`).
    pub fn get_all_leaves(&self) -> Vec<&TreeItem> {
        if self.children.is_empty() {
            return vec![self];
        }
        self.children
            .iter()
            .flat_map(TreeItem::get_all_leaves)
            .collect()
    }

    /// All downloadable items, excluding structural tree nodes
    /// (`get_all_children`).
    pub fn get_all_children(&self) -> Vec<&TreeItem> {
        self.children
            .iter()
            .flat_map(|c| {
                if c.children.is_empty() && !c.has_attribute(Attribute::TREE_NODE_BIT) {
                    vec![c]
                } else {
                    c.get_all_children()
                }
            })
            .collect()
    }

    /// Serialize the episode fields a Download Task consumes (`to_dict`).
    pub fn to_dict(&self) -> Value {
        let mut data = json!({
            "attribute": self.attribute.0,
            "episode_id": self.episode_id,
            "episode_number": self.episode_number,
            "aid": self.aid,
            "badge": self.badge,
            "bvid": self.bvid,
            "cid": self.cid,
            "sid": self.sid,
            "cover": self.cover,
            "duration": self.duration,
            "ep_id": self.ep_id,
            "number": self.number.to_json(),
            "pubtime": self.pubtime,
            "favtime": self.favtime,
            "viewtime": self.viewtime,
            "part_number": self.part_number,
            "related_titles": self.related_titles,
            "title": self.title,
            "url": self.url,
            "episode_plot": self.episode_plot,
        });

        if !self.uploader.is_empty() {
            data["uploader_info"] = json!({
                "uploader": self.uploader,
                "uploader_uid": self.uploader_uid,
            });
        }

        if self.section_id != 0 {
            data["course_id"] = json!(self.course_id);
            data["lesson_id"] = json!(self.lesson_id);
            data["item_id"] = json!(self.item_id);
            data["section_id"] = json!(self.section_id);
        }

        data
    }
}

/// Global episode side-data table (`EpisodeData`). Keyed by uuid; entries
/// referenced from tree nodes via `episode_id`.
pub struct EpisodeData {
    table: RwLock<HashMap<String, Value>>,
    active_parsers: AtomicU32,
}

impl Default for EpisodeData {
    fn default() -> Self {
        Self::new()
    }
}

impl EpisodeData {
    pub fn new() -> Self {
        Self {
            table: RwLock::new(HashMap::new()),
            active_parsers: AtomicU32::new(0),
        }
    }

    /// Register a fresh episode and return its id (`add_episode`).
    pub fn add_episode(&self) -> String {
        let id = Uuid::new_v4().to_string();
        self.table.write().insert(id.clone(), Map::new().into());
        id
    }

    /// Side data of an episode, empty object when unknown.
    pub fn get_episode_data(&self, episode_id: &str) -> Value {
        self.table
            .read()
            .get(episode_id)
            .cloned()
            .unwrap_or_else(|| Map::new().into())
    }

    /// Side data of an episode, `None` when unknown.
    pub fn try_get_episode_data(&self, episode_id: &str) -> Option<Value> {
        self.table.read().get(episode_id).cloned()
    }

    /// Overwrite an episode's side data (creates the entry when absent).
    pub fn set_episode_data(&self, episode_id: &str, data: Value) {
        self.table.write().insert(episode_id.to_owned(), data);
    }

    /// Drop all side data (`clear_cache`). Skipped while any parser is
    /// mid-flight: another concurrent parse may hold references to entries
    /// this would erase; the next exclusive parse reclaims them.
    pub fn clear_cache(&self) {
        if self.active_parsers.load(Ordering::SeqCst) > 0 {
            return;
        }
        self.table.write().clear();
    }

    /// RAII guard registering this parse as active (`parsing`).
    pub fn parsing(&self) -> ActiveParsers<'_> {
        self.active_parsers.fetch_add(1, Ordering::SeqCst);
        ActiveParsers { inner: self }
    }
}

/// Guard returned by [`EpisodeData::parsing`].
pub struct ActiveParsers<'a> {
    inner: &'a EpisodeData,
}

impl Drop for ActiveParsers<'_> {
    fn drop(&mut self) {
        self.inner.active_parsers.fetch_sub(1, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Check-state propagation and parsing-gate extensions (ticket 09).
//
// The Python tree carries a parent pointer, so `set_checked_state` on a
// child aggregates the ancestors directly. This value tree has none, so
// child→parent aggregation is an explicit root-side operation
// ([`TreeItem::set_child_checked_state`]); downward propagation is
// unchanged.
// ---------------------------------------------------------------------------

impl TreeItem {
    /// Set this node's check state, propagating down the whole subtree
    /// (`set_checked_state`).
    pub fn set_checked_state(&mut self, state: CheckState) {
        if self.checked == state {
            return;
        }
        self.checked = state;
        if matches!(state, CheckState::Checked | CheckState::Unchecked) {
            self.propagate_down(state);
        }
    }

    fn propagate_down(&mut self, state: CheckState) {
        self.checked = state;
        for child in &mut self.children {
            child.propagate_down(state);
        }
    }

    /// Bottom-up check-state recompute of this subtree
    /// (`refresh_check_state`); leaf states are taken as given.
    pub fn refresh_check_state(&mut self) -> CheckState {
        if self.children.is_empty() {
            return self.checked;
        }
        let states: Vec<CheckState> = self
            .children
            .iter_mut()
            .map(TreeItem::refresh_check_state)
            .collect();
        self.checked = if states.iter().all(|s| *s == CheckState::Checked) {
            CheckState::Checked
        } else if states.iter().all(|s| *s == CheckState::Unchecked) {
            CheckState::Unchecked
        } else {
            CheckState::PartiallyChecked
        };
        self.checked
    }

    /// Checked downloadable leaves, excluding structural tree nodes
    /// (`get_all_checked_children`).
    pub fn get_all_checked_children(&self) -> Vec<&TreeItem> {
        self.children
            .iter()
            .flat_map(|c| {
                if c.children.is_empty() {
                    if c.checked == CheckState::Checked
                        && !c.has_attribute(Attribute::TREE_NODE_BIT)
                    {
                        vec![c]
                    } else {
                        Vec::new()
                    }
                } else {
                    c.get_all_checked_children()
                }
            })
            .collect()
    }

    /// Change a child's check state and refresh this node's aggregated
    /// state — the value-tree equivalent of the Python child→parent
    /// propagation.
    pub fn set_child_checked_state(&mut self, row: usize, state: CheckState) {
        if let Some(child) = self.children.get_mut(row) {
            child.set_checked_state(state);
        }
        self.refresh_check_state();
    }
}

impl EpisodeData {
    /// RAII guard registering this parse as active (`parsing`). When
    /// `clear_cache` is set and no other parse is active, the table is
    /// cleared in the same critical section as the registration, so a
    /// parse squeezed in between clear and register cannot be mistaken
    /// for clearable.
    pub fn parsing_with_clear(&self, clear_cache: bool) -> ActiveParsers<'_> {
        let mut table = self.table.write();
        if clear_cache && self.active_parsers.load(Ordering::SeqCst) == 0 {
            table.clear();
        }
        self.active_parsers.fetch_add(1, Ordering::SeqCst);
        drop(table);
        ActiveParsers { inner: self }
    }

    /// Keys present in the table (test observability).
    pub fn episode_ids(&self) -> Vec<String> {
        self.table.read().keys().cloned().collect()
    }

    /// Number of currently active parsers (Python's `_active_parsers`).
    pub fn active_parsers(&self) -> u32 {
        self.active_parsers.load(Ordering::SeqCst)
    }

    /// Reset the table and the active-parser counter (test seam for the
    /// Python fixture's `EpisodeData.table.clear()` +
    /// `_active_parsers = 0`).
    pub fn reset(&self) {
        let mut table = self.table.write();
        self.active_parsers.store(0, Ordering::SeqCst);
        table.clear();
    }
}
