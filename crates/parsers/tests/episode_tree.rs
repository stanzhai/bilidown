//! Port of `test/test_episode_tree.py`.

use std::sync::{Arc, Barrier};

use serde_json::{json, Value};

use parsers::tree::{Attribute, CheckState, EpisodeData, Number, TreeItem};

fn build_tree() -> TreeItem {
    let mut root = TreeItem::root(Number::Empty, "root");
    root.set_attribute(Attribute::TREE_NODE_BIT);
    for i in 0..3 {
        let mut child = TreeItem::root(Number::Empty, &format!("child-{i}"));
        child.set_attribute(Attribute::VIDEO_BIT);
        root.add_child(child);
    }
    root
}

#[test]
fn checking_parent_checks_all_children() {
    let mut root = build_tree();
    root.set_checked_state(CheckState::Checked);
    assert_eq!(root.checked, CheckState::Checked);
    for child in &root.children {
        assert_eq!(child.checked, CheckState::Checked);
    }
    assert_eq!(root.get_all_checked_children().len(), 3);
}

#[test]
fn unchecking_parent_unchecks_all_children() {
    let mut root = build_tree();
    root.set_checked_state(CheckState::Checked);
    root.set_checked_state(CheckState::Unchecked);
    assert_eq!(root.checked, CheckState::Unchecked);
    for child in &root.children {
        assert_eq!(child.checked, CheckState::Unchecked);
    }
    assert_eq!(root.get_all_checked_children().len(), 0);
}

#[test]
fn partially_checked_child_makes_parent_partial() {
    let mut root = build_tree();
    root.set_child_checked_state(0, CheckState::Checked);
    assert_eq!(root.checked, CheckState::PartiallyChecked);
    assert_eq!(root.children[0].checked, CheckState::Checked);
    assert_eq!(root.children[1].checked, CheckState::Unchecked);
}

#[test]
fn all_children_checked_makes_parent_checked() {
    let mut root = build_tree();
    for i in 0..3 {
        root.set_child_checked_state(i, CheckState::Checked);
    }
    assert_eq!(root.checked, CheckState::Checked);
    assert_eq!(root.get_all_checked_children().len(), 3);
}

#[test]
fn unchecking_one_child_downgrades_parent_to_partial() {
    let mut root = build_tree();
    root.set_checked_state(CheckState::Checked);
    root.set_child_checked_state(1, CheckState::Unchecked);
    assert_eq!(root.checked, CheckState::PartiallyChecked);
    assert_eq!(root.get_all_checked_children().len(), 2);
}

#[test]
fn multi_level_propagation() {
    let mut root = build_tree();
    let mut section = TreeItem::root(Number::Empty, "section");
    section.set_attribute(Attribute::TREE_NODE_BIT);
    for i in 0..2 {
        let mut child = TreeItem::root(Number::Empty, &format!("g-{i}"));
        child.set_attribute(Attribute::VIDEO_BIT);
        section.add_child(child);
    }
    root.add_child(section);

    root.set_checked_state(CheckState::Checked);
    assert_eq!(root.get_all_checked_children().len(), 5);

    let section = root.children.last_mut().unwrap();
    section.set_child_checked_state(0, CheckState::Unchecked);
    assert_eq!(section.checked, CheckState::PartiallyChecked);
    root.refresh_check_state();
    assert_eq!(root.checked, CheckState::PartiallyChecked);
    assert_eq!(root.get_all_checked_children().len(), 4);
}

#[test]
fn refresh_check_state_matches_incremental() {
    let mut incremental = build_tree();
    for i in 0..3 {
        incremental.set_child_checked_state(i, CheckState::Checked);
    }

    let mut refreshed = build_tree();
    for child in &mut refreshed.children {
        child.checked = CheckState::Checked;
    }
    let state = refreshed.refresh_check_state();

    assert_eq!(state, incremental.checked);
    assert_eq!(state, CheckState::Checked);
    assert_eq!(
        refreshed.get_all_checked_children().len(),
        incremental.get_all_checked_children().len()
    );
}

#[test]
fn get_all_leaves_returns_all_children() {
    let root = build_tree();
    let leaves = root.get_all_leaves();
    assert_eq!(leaves.len(), 3);
    for (i, leaf) in leaves.iter().enumerate() {
        assert_eq!(leaf.title, format!("child-{i}"));
    }
}

#[test]
fn clear_cache_clears_table_when_idle() {
    let data = EpisodeData::default();
    data.set_episode_data("abc", json!({"a": 1}));
    data.clear_cache();
    assert!(data.try_get_episode_data("abc").is_none());
}

#[test]
fn clear_cache_skipped_inside_parsing() {
    let data = EpisodeData::default();
    data.set_episode_data("abc", json!({"a": 1}));
    let guard = data.parsing();
    data.clear_cache();
    assert!(data.try_get_episode_data("abc").is_some());
    drop(guard);
}

#[test]
fn parsing_with_clear_clears_when_idle() {
    let data = EpisodeData::default();
    data.set_episode_data("abc", json!({"a": 1}));
    let guard = data.parsing_with_clear(true);
    assert!(data.try_get_episode_data("abc").is_none());
    drop(guard);
}

#[test]
fn nested_parsing_without_clear_preserves_data() {
    let data = EpisodeData::default();
    let outer = data.parsing_with_clear(true);
    data.set_episode_data("abc", json!({"a": 1}));
    let inner = data.parsing_with_clear(false);
    assert_eq!(data.active_parsers(), 2);
    assert!(data.try_get_episode_data("abc").is_some());
    drop(inner);
    assert_eq!(data.active_parsers(), 1);
    assert!(data.try_get_episode_data("abc").is_some());
    drop(outer);
}

#[test]
fn parsing_counter_restored_on_panic() {
    let data = EpisodeData::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = data.parsing();
        assert_eq!(data.active_parsers(), 1);
        panic!("boom");
    }));
    assert!(result.is_err());
    assert_eq!(data.active_parsers(), 0);
}

#[test]
fn concurrent_parses_do_not_erase_each_other() {
    let data = Arc::new(EpisodeData::default());
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();

    for owner in ["A", "B"] {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let episode_id = data.add_episode();
            let _guard = data.parsing_with_clear(true);
            data.set_episode_data(&episode_id, json!({"owner": owner}));
            barrier.wait();
            // Give the sibling a moment to finish its own write.
            barrier.wait();
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }

    let owners: Vec<String> = ["A", "B"]
        .into_iter()
        .map(|o| o.to_owned())
        .collect();
    let mut seen = Vec::new();
    for id in data.episode_ids() {
        if let Some(Value::Object(map)) = data.try_get_episode_data(&id) {
            if let Some(owner) = map.get("owner").and_then(Value::as_str) {
                if owners.contains(&owner.to_owned()) {
                    seen.push(owner.to_owned());
                }
            }
        }
    }
    seen.sort();
    assert_eq!(seen, vec!["A".to_owned(), "B".to_owned()]);
}

