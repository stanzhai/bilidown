//! Parse-list view model (port of the state half of
//! `gui/component/parse_list/tree_view.py` — everything a GUI kit needs to
//! render the list: the tree, the pending-locate state, the checked set,
//! the keyword filter).
//!
//! Ports the semantics pinned by `test/test_parse_list_view.py`:
//! a play-page link points at one item of a collection; that item may land
//! on any page of the parse. The first page that contains it checks it
//! immediately and clears the pending locate data; pages before it keep
//! the data pending, re-checking on every append; a new parse (`clear_tree`
//! / `update_tree`) drops the stale pending data.

use crate::tree::{Attribute, CheckState, TreeItem};

/// Parse-list state. UI events (select/scroll) are the view's job; this
/// owns the model side.
pub struct ParseListView {
    root: TreeItem,
    pending_episode_data: Option<(String, String)>,
    current_episode_bvid: Option<String>,
    search_keyword: String,
}

/// What a locate attempt did.
#[derive(Debug, PartialEq, Eq)]
pub enum ViewError {
    /// The locate key names no field of a tree item.
    UnknownField(String),
}

/// Field of [`TreeItem`] a locate key can address. Python's
/// `getattr(item, key)` accepts any field name; these are the keys the
/// parsers actually emit (`parse_video` → cid, bangumi/cheese → ep_id,
/// lesson → section_id, list → bvid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocateField {
    Bvid,
    EpisodeId,
    Cid,
    EpId,
    SectionId,
}

impl LocateField {
    fn value_of(self, item: &TreeItem) -> String {
        match self {
            LocateField::Bvid => item.bvid.clone(),
            LocateField::EpisodeId => item.episode_id.clone(),
            LocateField::Cid => item.cid.to_string(),
            LocateField::EpId => item.ep_id.to_string(),
            LocateField::SectionId => item.section_id.to_string(),
        }
    }
}

impl std::str::FromStr for LocateField {
    type Err = ViewError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bvid" => Ok(LocateField::Bvid),
            "episode_id" => Ok(LocateField::EpisodeId),
            "cid" => Ok(LocateField::Cid),
            "ep_id" => Ok(LocateField::EpId),
            "section_id" => Ok(LocateField::SectionId),
            other => Err(ViewError::UnknownField(other.to_owned())),
        }
    }
}

impl Default for ParseListView {
    fn default() -> Self {
        Self::new()
    }
}

impl ParseListView {
    pub fn new() -> Self {
        Self {
            root: TreeItem::empty(),
            pending_episode_data: None,
            current_episode_bvid: None,
            search_keyword: String::new(),
        }
    }

    /// Replace the whole tree (`update_tree`). `current_episode_data` is
    /// `(field, value)` — the item the link points at, when it does.
    /// The target is checked immediately when it is on this page;
    /// otherwise the data stays pending for later appends.
    pub fn update_tree(
        &mut self,
        root_node: TreeItem,
        current_episode_data: Option<(String, String)>,
    ) -> Result<(), ViewError> {
        self.root = root_node;
        self.current_episode_bvid = None;

        let target = self.locate_by_episode_data(current_episode_data.as_ref(), true)?;
        self.pending_episode_data = if target.is_none() {
            current_episode_data
        } else {
            None
        };

        Ok(())
    }

    /// Append one page's items (`append_nodes`), then retry the pending
    /// locate if one is outstanding.
    pub fn append_nodes(&mut self, nodes: Vec<TreeItem>) -> Result<(), ViewError> {
        // New nodes hang off the first category node (the parse appends
        // into the same wrapper → category structure as page one).
        if let Some(category) = self.root.children.first_mut() {
            for node in nodes {
                category.add_child(node);
            }
        }

        let pending = self.pending_episode_data.clone();
        if let Some(target) = self.locate_by_episode_data(pending.as_ref(), true)? {
            self.pending_episode_data = None;
            self.current_episode_bvid = Some(target);
        }
        Ok(())
    }

    /// Drop the tree *and* the pending locate (`clear_tree`): a target the
    /// previous parse never found must not be checked in the next one.
    pub fn clear_tree(&mut self) {
        self.root = TreeItem::empty();
        self.pending_episode_data = None;
        self.current_episode_bvid = None;
        self.search_keyword.clear();
    }

    /// `locate_to_item_by_episode_data`: first item whose `key` field
    /// equals `value`; checks it when `check`. No data or empty value →
    /// no match, no panic.
    pub fn locate_by_episode_data(
        &mut self,
        current_episode_data: Option<&(String, String)>,
        check: bool,
    ) -> Result<Option<String>, ViewError> {
        let Some((key, value)) = current_episode_data else {
            return Ok(None);
        };
        // An empty value would match the default of every item.
        if value.is_empty() {
            return Ok(None);
        }

        let field: LocateField = key.parse()?;

        for item in self.root.get_all_children() {
            // Recurse interiorally: borrow conflict avoidance (one mutable
            // walk that checks on match).
            let _ = item;
        }

        let found = Self::find_and_check(&mut self.root, field, value, check);
        Ok(found)
    }

    fn find_and_check(
        node: &mut TreeItem,
        field: LocateField,
        value: &str,
        check: bool,
    ) -> Option<String> {
        for item in node.children.iter_mut() {
            if !item.has_attribute(Attribute::TREE_NODE_BIT) && field.value_of(item) == value {
                if check {
                    item.checked = CheckState::Checked;
                }
                // Identity for `current_episode_bvid` / preview dispatch:
                // the located item's own bvid (multi-P parts share it, but
                // preview fetch then uses the checked item directly).
                return Some(item.bvid.clone());
            }
            if let Some(hit) = Self::find_and_check(item, field, value, check) {
                return Some(hit);
            }
        }
        None
    }

    /// All downloadable items (`get_all_items`).
    pub fn get_all_items(&self) -> Vec<&TreeItem> {
        self.root.get_all_children()
    }

    /// Checked items, in tree order (`get_checked_items`).
    pub fn get_checked_items(&self) -> Vec<&TreeItem> {
        self.get_all_items()
            .into_iter()
            .filter(|item| item.checked == CheckState::Checked)
            .collect()
    }

    /// Check/uncheck everything (`check_all_items`) — root state, children
    /// follow at render time; we just set every leaf directly.
    pub fn check_all_items(&mut self, check: bool) {
        let state = if check {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        };
        Self::set_all(&mut self.root, state);
    }

    fn set_all(node: &mut TreeItem, state: CheckState) {
        node.checked = state;
        for child in node.children.iter_mut() {
            Self::set_all(child, state);
        }
    }

    /// Check specific items (`check_items`, the search "check all
    /// matches" action).
    pub fn check_items(&mut self, bvids: &[&str]) {
        for item in self.root.get_all_children() {
            let _ = item;
        }
        Self::check_matching(&mut self.root, bvids);
    }

    fn check_matching(node: &mut TreeItem, bvids: &[&str]) {
        for item in node.children.iter_mut() {
            if bvids.contains(&item.bvid.as_str()) {
                item.checked = CheckState::Checked;
            }
            Self::check_matching(item, bvids);
        }
    }

    /// Batch-selection by episode number (the misc dialog's
    /// "1,3,5-10" grammar already parsed by the caller).
    pub fn check_numbers(&mut self, numbers: &[i64]) {
        fn walk(node: &mut TreeItem, numbers: &[i64]) {
            for item in node.children.iter_mut() {
                if let crate::Number::Int(n) = &item.number {
                    if numbers.contains(n) {
                        item.checked = CheckState::Checked;
                    }
                }
                walk(item, numbers);
            }
        }
        walk(&mut self.root, numbers);
    }

    /// Filter by title (`search_keywords`): returns the matching items in
    /// tree order; `None`/empty keyword clears the filter. The match list
    /// is what the search widget's prev/next navigation walks.
    pub fn search_keywords(&mut self, keywords: Option<&str>) -> Vec<&TreeItem> {
        match keywords {
            Some(k) if !k.is_empty() => {
                self.search_keyword = k.to_owned();
            }
            _ => {
                self.search_keyword.clear();
                return Vec::new();
            }
        }
        self.root.search_items(&self.search_keyword)
    }

    /// The keyword currently filtering the list.
    pub fn search_keyword(&self) -> &str {
        &self.search_keyword
    }

    /// The item the link points at, when located (`_current_episode_item`
    /// identity — the bvid).
    pub fn current_episode_bvid(&self) -> Option<&str> {
        self.current_episode_bvid.as_deref()
    }

    /// The invisible wrapper root (display rows are its children). Read
    /// seam for UI layers that flatten the tree into display rows.
    pub fn root(&self) -> &TreeItem {
        &self.root
    }

    /// Mutable root seam: callers must re-run the check-state aggregation
    /// themselves after leaf edits (`refresh_check_state`).
    pub fn root_mut(&mut self) -> &mut TreeItem {
        &mut self.root
    }

    /// Pending locate data (test seam mirroring
    /// `ParseTreeView._pending_episode_data`).
    pub fn pending_episode_data(&self) -> Option<&(String, String)> {
        self.pending_episode_data.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Number;

    fn make_item(bvid: &str) -> TreeItem {
        let mut item = TreeItem::empty();
        item.bvid = bvid.to_owned();
        item.title = bvid.to_owned();
        item
    }

    fn make_page(bvids: &[&str]) -> TreeItem {
        let mut root = TreeItem::empty();
        let mut node = TreeItem::root(Number::Text("合集列表".into()), "测试合集");
        node.set_attribute(Attribute::TREE_NODE_BIT);
        for bvid in bvids {
            node.add_child(make_item(bvid));
        }
        root.add_child(node);
        root
    }

    fn checked(view: &ParseListView) -> Vec<String> {
        view.get_checked_items()
            .into_iter()
            .map(|i| i.bvid.clone())
            .collect()
    }

    #[test]
    fn target_on_the_first_page_is_checked_immediately() {
        let mut view = ParseListView::new();
        view.update_tree(
            make_page(&["BV_target", "BV_other"]),
            Some(("bvid".into(), "BV_target".into())),
        )
        .unwrap();

        assert_eq!(checked(&view), ["BV_target"]);
        assert!(view.pending_episode_data().is_none());
    }

    #[test]
    fn target_on_a_later_page_is_checked_when_it_arrives() {
        let mut view = ParseListView::new();
        view.update_tree(
            make_page(&["BV_first", "BV_second"]),
            Some(("bvid".into(), "BV_target".into())),
        )
        .unwrap();

        assert!(
            checked(&view).is_empty(),
            "第一页里没有目标，不该勾选任何条目"
        );
        assert_eq!(
            view.pending_episode_data(),
            Some(&("bvid".to_owned(), "BV_target".to_owned())),
            "要把定位信息留着等后续分页"
        );

        view.append_nodes(vec![make_item("BV_target")]).unwrap();

        assert_eq!(checked(&view), ["BV_target"]);
        assert_eq!(
            view.current_episode_bvid(),
            Some("BV_target"),
            "媒体信息预览要指向这一项"
        );
        assert!(
            view.pending_episode_data().is_none(),
            "定位到了就不该再留着"
        );
    }

    #[test]
    fn pending_target_is_dropped_when_a_new_parse_starts() {
        let mut view = ParseListView::new();
        view.update_tree(
            make_page(&["BV_first"]),
            Some(("bvid".into(), "BV_target".into())),
        )
        .unwrap();
        view.clear_tree();

        assert!(view.pending_episode_data().is_none());

        // 上一次没找到的目标不该在这一棵树里被勾上
        view.append_nodes(vec![make_item("BV_target")]).unwrap();

        assert!(checked(&view).is_empty());
    }

    #[test]
    fn no_locate_data_means_no_checking() {
        // 个人空间、收藏夹这类链接不指向具体条目，追加分页时也不能凭空勾选
        let mut view = ParseListView::new();
        view.update_tree(make_page(&["BV_first"]), None).unwrap();
        view.append_nodes(vec![make_item("BV_second")]).unwrap();

        assert!(checked(&view).is_empty());
    }
}
