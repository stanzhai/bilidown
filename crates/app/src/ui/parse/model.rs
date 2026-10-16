//! Parse-screen view model: the app-side wrapper over
//! `parsers::view::ParseListView` that adds the behaviors the gpui tree
//! needs but the shared view model lacks (port of the Python
//! `ParseTreeView` methods the widget owned).
//!
//! Two layers of "tree item" exist here:
//! - `parsers::tree::TreeItem` — the episode data model (checked state,
//!   numbers, badges).
//! - gpui-base `TreeItem` — pure `{id, label, children}` display rows
//!   built from the episode model via [`ParseViewModel::refresh_rows`].
//!
//! `ParseViewModel` owns the episode model plus the UI state the Python
//! interface kept in the widget (search matches + cursor, category label,
//! pagination flags), and rebuilds display rows on mutation.

use parsers::tree::{CheckState, Number, TreeItem as EpisodeItem};
use parsers::view::ParseListView;
use serde_json::Value;

/// One flattened display row: stable id + label. The id must be stable
/// across rebuilds so tree reveal/scroll works; `number` + title are
/// unique enough within one parse result, bvid disambiguates re-added
/// pages.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayRow {
    pub id: String,
    pub label: String,
    pub depth: usize,
    pub is_leaf: bool,
}

/// Stable per-node row id.
fn row_id(item: &EpisodeItem) -> String {
    let number = match &item.number {
        Number::Int(n) => n.to_string(),
        Number::Text(n) => n.clone(),
        Number::Empty => String::new(),
    };
    format!("{}|{}|{}", item.bvid, number, item.title)
}

/// Row label: number, title, badge.
pub fn row_label(item: &EpisodeItem) -> String {
    let number = match &item.number {
        Number::Int(n) => n.to_string(),
        Number::Text(n) => n.clone(),
        Number::Empty => String::new(),
    };
    match (number.is_empty(), item.badge.is_empty()) {
        (true, true) => item.title.clone(),
        (false, true) => format!("{number}  {}", item.title),
        (true, false) => format!("{}  [{}]", item.title, item.badge),
        (false, false) => format!("{number}  {}  [{}]", item.title, item.badge),
    }
}

/// "1,3,5-10" → [1,3,5..=10]. Port of `BatchSelectDialog.to_number_list`
/// (inclusive ranges, comma-separated). `None` on any malformed token
/// (Python's dialog validates before accept; here the caller checks
/// [`parse_ranges`] first and toasts).
pub fn parse_ranges(input: &str) -> Option<Vec<i64>> {
    let mut out = Vec::new();
    for part in input.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((start, end)) = part.split_once('-') {
            let start = start.trim().parse::<i64>().ok()?;
            let end = end.trim().parse::<i64>().ok()?;
            if end < start {
                return None;
            }
            out.extend(start..=end);
        } else {
            out.push(part.parse::<i64>().ok()?);
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// The view model over the parse list. Wraps `ParseListView` (the port of
/// the shared model half) and adds what only the gpui app needs.
pub struct ParseViewModel {
    pub list: ParseListView,
    /// Display rows for the current tree (rebuilt on mutation).
    rows: Vec<DisplayRow>,
    /// Localized category label, e.g. "Favorites".
    pub category_name: String,
    /// Raw category key (`ParseOutput.category`).
    pub category_key: String,
    /// In-page search: matching row indexes + cursor for jump next/prev.
    pub matches: Vec<usize>,
    pub match_cursor: Option<usize>,
    /// Pagination of the current parse result (`extra.pagination`).
    pub pagination: Option<PaginationState>,
    /// Whether the current url family supports server-side search.
    pub server_search: bool,
    /// Keyword already baked into the current url, if any.
    pub search_keyword: String,
    /// bvids already sent to download (rows render grayed out).
    pub downloaded: std::collections::HashSet<String>,
}

/// Pager state (`pagination_data`): page math + label.
#[derive(Debug, Clone, PartialEq)]
pub struct PaginationState {
    pub current_page: i64,
    pub total_pages: i64,
    pub total_items: i64,
}

impl PaginationState {
    pub fn from_extra(pagination: &parsers::list::Pagination) -> Self {
        Self {
            current_page: pagination.current_page,
            total_pages: pagination.total_pages,
            total_items: pagination.total_items,
        }
    }

    /// "{total_pages} total / {total_items} items" pager label.
    pub fn label(&self) -> String {
        format!("{} total / {} items", self.total_pages, self.total_items)
    }
}

impl Default for ParseViewModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ParseViewModel {
    pub fn new() -> Self {
        Self {
            list: ParseListView::new(),
            rows: Vec::new(),
            category_name: String::new(),
            category_key: String::new(),
            matches: Vec::new(),
            match_cursor: None,
            pagination: None,
            server_search: false,
            search_keyword: String::new(),
            downloaded: std::collections::HashSet::new(),
        }
    }

    /// Current display rows.
    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    /// Rebuild display rows from the episode model. Call after any model
    /// mutation that changes the tree shape.
    pub fn refresh_rows(&mut self) {
        let mut rows = Vec::new();
        flatten(self.list.root(), 0, &mut rows);
        self.rows = rows;
    }

    /// Checked / total counts for the item-count label.
    pub fn counts(&self) -> (usize, usize) {
        (
            self.list.get_checked_items().len(),
            self.list.get_all_items().len(),
        )
    }

    /// Item-count label: with selection
    /// `"{category_name} ({selected_count} selected, {total_count} total)"`
    /// else `"{category_name} ({total_count} total)"`.
    pub fn count_label(&self) -> String {
        let (checked, total) = self.counts();
        if checked > 0 {
            format!("{} ({} selected, {} total)", self.category_name, checked, total)
        } else {
            format!("{} ({} total)", self.category_name, total)
        }
    }

    /// Batch select: check every item whose `number` is in `numbers`
    /// (`ParseTreeView.batch_select`).
    pub fn batch_select(&mut self, numbers: &[i64]) {
        let bvids: Vec<String> = self
            .list
            .get_all_items()
            .iter()
            .filter(|item| matches!(item.number, Number::Int(n) if numbers.contains(&n)))
            .map(|item| item.bvid.clone())
            .collect();
        let refs: Vec<&str> = bvids.iter().map(String::as_str).collect();
        self.list.check_items(&refs);
    }

    /// Preview candidates: the link-target item (else the first), then
    /// backups with no badge preferred (`get_preview_candidates`,
    /// default limit 3) — returned as episode dicts for the previewer walk.
    pub fn preview_candidates(&self, limit: usize) -> Vec<Value> {
        let all = self.list.get_all_items();
        let Some(first) = all.first() else {
            return Vec::new();
        };
        let current = self.list.current_episode_bvid();
        let primary = match current {
            Some(bvid) => all.iter().find(|i| i.bvid == bvid).unwrap_or(first),
            None => first,
        };
        let mut backups: Vec<&EpisodeItem> = all
            .iter()
            .filter(|i| !std::ptr::eq(*i, primary))
            .copied()
            .collect();
        // Badged items are often paywalled/charged — sort them last.
        backups.sort_by_key(|i| !i.badge.is_empty());
        let mut out = vec![primary.to_dict()];
        out.extend(
            backups
                .into_iter()
                .take(limit.saturating_sub(1))
                .map(|i| i.to_dict()),
        );
        out
    }

    /// In-page search: store the keyword, refresh matches as row indexes,
    /// reset the cursor to the first match. Returns the match count.
    pub fn search(&mut self, keyword: Option<&str>) -> usize {
        let matched: Vec<String> = self
            .list
            .search_keywords(keyword)
            .iter()
            .map(|i| row_id(i))
            .collect();
        self.search_keyword = keyword.unwrap_or("").to_string();
        self.matches = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matched.iter().any(|id| id == &row.id))
            .map(|(ix, _)| ix)
            .collect();
        self.match_cursor = if self.matches.is_empty() { None } else { Some(0) };
        self.matches.len()
    }

    /// Advance/wrap the match cursor, returning the row index to reveal.
    pub fn next_match(&mut self) -> Option<usize> {
        self.step_match(1)
    }

    pub fn prev_match(&mut self) -> Option<usize> {
        self.step_match(-1)
    }

    fn step_match(&mut self, dir: i32) -> Option<usize> {
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        let cur = self.match_cursor.unwrap_or(0);
        let next = if dir > 0 {
            (cur + 1) % len
        } else {
            (cur + len - 1) % len
        };
        self.match_cursor = Some(next);
        Some(self.matches[next])
    }

    /// "{index} of {count}" match label; "No matches found" when empty.
    pub fn match_label(&self) -> String {
        match self.match_cursor {
            Some(cur) if !self.matches.is_empty() => {
                format!("{} of {}", cur + 1, self.matches.len())
            }
            _ => "No matches found".to_string(),
        }
    }

    /// Check all matched items (search-bar "Select All").
    pub fn check_matches(&mut self) {
        let ids: Vec<String> = self
            .matches
            .iter()
            .filter_map(|ix| self.rows.get(*ix).map(|r| r.id.clone()))
            .collect();
        if ids.is_empty() {
            return;
        }
        let bvids: Vec<String> = self
            .list
            .get_all_items()
            .iter()
            .filter(|i| ids.iter().any(|id| id == &row_id(i)))
            .map(|i| i.bvid.clone())
            .collect();
        let refs: Vec<&str> = bvids.iter().map(String::as_str).collect();
        self.list.check_items(&refs);
    }

    /// Checked episode dicts, marking them downloaded
    /// (`get_checked_items(to_dict=True, mark_as_downloaded=True)`).
    /// Downloaded tracking lives here (a bvid set) rather than on
    /// `parsers::TreeItem`, which has no such field; the set drives the
    /// gray-out rendering.
    pub fn checked_dicts_mark_downloaded(&mut self) -> Vec<Value> {
        let items: Vec<EpisodeItem> =
            self.list.get_checked_items().into_iter().cloned().collect();
        for item in &items {
            if !item.bvid.is_empty() {
                self.downloaded.insert(item.bvid.clone());
            }
        }
        items.iter().map(|i| i.to_dict()).collect()
    }

    /// Bangumi main-episode check (`_check_main_episodes_node`):
    /// check `root.children[0].children[0]` when present.
    pub fn check_main_episodes_node(&mut self) {
        let root = self.list.root_mut();
        if let Some(first) = root.children.first_mut() {
            if let Some(main) = first.children.first_mut() {
                main.set_checked_state(CheckState::Checked);
            }
        }
    }

    /// Auto-select per config (`apply_auto_select`): SELECT_ALL → check
    /// all; CONDITIONAL → per-category conditions; MANUAL → none.
    /// Config values are the `Behavior.auto_select_mode_` ints
    /// (0 manual / 1 select-all / 2 conditional).
    pub fn apply_auto_select(&mut self, cfg: &config::ConfigStore) {
        let mode = cfg
            .get("Behavior", "auto_select_mode_")
            .and_then(Value::as_i64)
            .unwrap_or(2);
        match mode {
            1 => self.list.check_all_items(true),
            2 => {
                let cond = |key: &str| {
                    cfg.get("Behavior", "auto_select_conditions")
                        .and_then(|v| v.get(key))
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        == 1
                };
                let (user_uploads, bangumi, other) =
                    (cond("user_uploads"), cond("bangumi"), cond("other"));
                match self.category_key.as_str() {
                    // User uploads: default already selects the linked
                    // video; only the select-all condition adds more.
                    "USER_UPLOADS" => {
                        if user_uploads {
                            self.list.check_all_items(true);
                        }
                    }
                    // Bangumi-family: default selects the linked episode;
                    // the condition selects the main-episodes node.
                    "ANIME" | "DOCUMENTARY" | "TV" | "CHN_ANIME" | "MOVIE" | "VARIETY" => {
                        if bangumi {
                            self.check_main_episodes_node();
                        }
                    }
                    // Courses: condition selects all.
                    "COURSE" | "MALL_COURSE" => {
                        if bangumi {
                            self.list.check_all_items(true);
                        }
                    }
                    // Everything else defaults to none; condition selects all.
                    _ => {
                        if other {
                            self.list.check_all_items(true);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Reset parse state (`reset_parse_list`).
    pub fn reset(&mut self) {
        self.list.clear_tree();
        self.rows.clear();
        self.matches.clear();
        self.match_cursor = None;
        self.category_name.clear();
        self.category_key.clear();
        self.pagination = None;
        self.server_search = false;
        self.search_keyword.clear();
    }
}

fn flatten(item: &EpisodeItem, depth: usize, out: &mut Vec<DisplayRow>) {
    for child in &item.children {
        out.push(DisplayRow {
            id: row_id(child),
            label: row_label(child),
            depth,
            is_leaf: child.children.is_empty(),
        });
        flatten(child, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parsers::tree::{Attribute, CheckState, Number, TreeItem};

    fn item(bvid: &str, number: i64, title: &str, badge: &str) -> TreeItem {
        let mut it = TreeItem::empty();
        it.bvid = bvid.into();
        it.title = title.into();
        it.badge = badge.into();
        it.number = Number::Int(number);
        it
    }

    fn page(items: Vec<TreeItem>) -> TreeItem {
        let mut root = TreeItem::empty();
        let mut node = TreeItem::root(Number::Text("合集列表".into()), "测试合集");
        node.set_attribute(Attribute::TREE_NODE_BIT);
        for mut it in items {
            it.number = Number::Int(match it.number {
                Number::Int(n) => n,
                _ => 0,
            });
            node.add_child(it);
        }
        root.add_child(node);
        root
    }

    fn model_with(items: Vec<TreeItem>) -> ParseViewModel {
        let mut m = ParseViewModel::new();
        m.list.update_tree(page(items), None).unwrap();
        m.refresh_rows();
        m
    }

    fn cfg_with(mode: i64, user_uploads: i64, bangumi: i64, other: i64) -> config::ConfigStore {
        let mut cfg = config::ConfigStore::open().unwrap();
        cfg.set("Behavior", "auto_select_mode_", Value::from(mode));
        cfg.set(
            "Behavior",
            "auto_select_conditions",
            serde_json::json!({"user_uploads": user_uploads, "bangumi": bangumi, "other": other}),
        );
        cfg
    }

    #[test]
    fn ranges_grammar_accepts_single_range_and_mix() {
        assert_eq!(parse_ranges("1"), Some(vec![1]));
        assert_eq!(parse_ranges("1, 3"), Some(vec![1, 3]));
        assert_eq!(parse_ranges("1,3,5-7"), Some(vec![1, 3, 5, 6, 7]));
        assert_eq!(parse_ranges(" 2 - 4 "), Some(vec![2, 3, 4]));
    }

    #[test]
    fn ranges_grammar_rejects_malformed_tokens() {
        assert_eq!(parse_ranges(""), None);
        assert_eq!(parse_ranges(",,,"), None);
        assert_eq!(parse_ranges("x"), None);
        assert_eq!(parse_ranges("5-2"), None, "反向区间必须整体拒绝");
        assert_eq!(parse_ranges("1,x,3"), None, "一个坏 token 毁掉整个输入");
        assert!(!parse_ranges("3-").is_some());
    }

    #[test]
    fn count_label_formats_selected_and_plain() {
        let mut m = model_with(vec![
            item("BV1", 1, "a", ""),
            item("BV2", 2, "b", ""),
        ]);
        m.category_name = "Favorites".into();
        assert_eq!(m.count_label(), "Favorites (2 total)");
        m.batch_select(&[2]);
        assert_eq!(m.count_label(), "Favorites (1 selected, 2 total)");
    }

    #[test]
    fn batch_select_checks_only_matching_numbers() {
        let mut m = model_with(vec![
            item("BV1", 1, "a", ""),
            item("BV2", 2, "b", ""),
            item("BV3", 3, "c", ""),
        ]);
        m.batch_select(&[1, 3]);
        let checked: Vec<String> = m.list.get_checked_items().iter().map(|i| i.bvid.clone()).collect();
        assert_eq!(checked, ["BV1", "BV3"]);
    }

    #[test]
    fn preview_candidates_primary_then_unbadged_backups() {
        let m = model_with(vec![
            item("BV1", 1, "a", "会员"),
            item("BV2", 2, "b", ""),
            item("BV3", 3, "c", ""),
            item("BV4", 4, "d", "付费"),
        ]);
        let dicts = m.preview_candidates(3);
        let ids: Vec<&str> = dicts
            .iter()
            .filter_map(|d| d.get("bvid").and_then(Value::as_str))
            .collect();
        // 无 current episode → 第一项为主选；无徽章的排前面
        assert_eq!(ids, ["BV1", "BV2", "BV3"]);
    }

    #[test]
    fn search_matches_and_cursor_wraps() {
        let mut m = model_with(vec![
            item("BV1", 1, "first ep", ""),
            item("BV2", 2, "other", ""),
            item("BV3", 3, "second ep", ""),
        ]);
        assert_eq!(m.search(Some("ep")), 2);
        assert_eq!(m.match_label(), "1 of 2");
        assert_eq!(m.next_match(), Some(m.matches[1]));
        assert_eq!(m.match_label(), "2 of 2");
        assert_eq!(m.next_match(), Some(m.matches[0]), "到尾后要回绕");
        assert_eq!(m.prev_match(), Some(m.matches[1]), "回头也要回绕");
        assert_eq!(m.search(Some("zzz")), 0);
        assert_eq!(m.match_label(), "No matches found");
    }

    #[test]
    fn check_matches_checks_only_search_hits() {
        let mut m = model_with(vec![
            item("BV1", 1, "keep ep", ""),
            item("BV2", 2, "drop", ""),
        ]);
        m.search(Some("ep"));
        m.check_matches();
        let checked: Vec<String> = m.list.get_checked_items().iter().map(|i| i.bvid.clone()).collect();
        assert_eq!(checked, ["BV1"]);
    }

    #[test]
    fn checked_dicts_mark_downloaded_records_bvids() {
        let mut m = model_with(vec![item("BV1", 1, "a", ""), item("BV2", 2, "b", "")]);
        m.batch_select(&[1]);
        let dicts = m.checked_dicts_mark_downloaded();
        assert_eq!(dicts.len(), 1);
        assert!(m.downloaded.contains("BV1"));
        assert!(!m.downloaded.contains("BV2"));
    }

    #[test]
    fn auto_select_matrix() {
        // 手动模式：什么都不选
        let mut m = model_with(vec![item("BV1", 1, "a", "")]);
        m.category_key = "USER_UPLOADS".into();
        m.apply_auto_select(&cfg_with(0, 1, 1, 1));
        assert!(m.list.get_checked_items().is_empty());

        // 全选模式：全部勾上
        let mut m = model_with(vec![item("BV1", 1, "a", ""), item("BV2", 2, "b", "")]);
        m.category_key = "WATCH_LATER".into();
        m.apply_auto_select(&cfg_with(1, 0, 0, 0));
        assert_eq!(m.list.get_checked_items().len(), 2);

        // 条件模式 × 各分类
        let mut m = model_with(vec![item("BV1", 1, "a", "")]);
        m.category_key = "USER_UPLOADS".into();
        m.apply_auto_select(&cfg_with(2, 1, 0, 0));
        assert_eq!(m.list.get_checked_items().len(), 1, "user_uploads 条件开启 → 全选");
        let mut m = model_with(vec![item("BV1", 1, "a", "")]);
        m.apply_auto_select(&cfg_with(2, 0, 0, 0));
        assert!(m.list.get_checked_items().is_empty(), "user_uploads 条件关闭 → 不加选");

        let mut m = model_with(vec![item("BV1", 1, "a", "")]);
        m.category_key = "FIV".into();
        m.apply_auto_select(&cfg_with(2, 0, 0, 1));
        assert_eq!(m.list.get_checked_items().len(), 1, "other 条件开启 → 全选");
    }

    #[test]
    fn check_main_episodes_node_checks_first_grandchild() {
        let mut root = TreeItem::empty();
        let mut season = TreeItem::root(Number::Text("正片".into()), "正片");
        let mut main = TreeItem::root(Number::Text("main".into()), "正片");
        main.add_child(item("BV_main", 1, "EP1", ""));
        season.add_child(main);
        root.add_child(season);

        let mut m = ParseViewModel::new();
        m.list.update_tree(root, None).unwrap();
        m.check_main_episodes_node();
        let node = &m.list.root().children[0].children[0];
        assert_eq!(node.checked, CheckState::Checked, "勾的正是 main-episodes 节点");
        // set_checked_state 向下传播：正片节点下的条目全选
        let checked: Vec<String> =
            m.list.get_checked_items().iter().map(|i| i.bvid.clone()).collect();
        assert_eq!(checked, ["BV_main"]);
    }

    #[test]
    fn reset_clears_everything() {
        let mut m = model_with(vec![item("BV1", 1, "a", "")]);
        m.category_name = "X".into();
        m.category_key = "USER_UPLOADS".into();
        m.pagination = Some(PaginationState {
            current_page: 1,
            total_pages: 3,
            total_items: 30,
        });
        m.search(Some("a"));
        m.reset();
        assert!(m.rows().is_empty());
        assert!(m.matches.is_empty() && m.match_cursor.is_none());
        assert!(m.pagination.is_none());
        assert!(m.category_name.is_empty() && m.category_key.is_empty());
    }

    #[test]
    fn pagination_label_formats_totals() {
        let p = PaginationState {
            current_page: 2,
            total_pages: 5,
            total_items: 47,
        };
        assert_eq!(p.label(), "5 total / 47 items");
    }
}
