//! test_task_naming_snapshot.py — the naming rule frozen at creation time is
//! the only one the task ever uses (queued tasks survive later global choices,
//! mixed batches pick each entry's own type rule); ported verbatim.

use std::collections::HashMap;

use naming::conventions::ConventionType;
use naming::formatter::{attribute, convention_type};
use naming::naming_rules::RuleStore;
use naming::task_naming::{
    freeze_naming_rule, update_episode_info, update_file_name_info, EpisodeData, NamingOptions,
    NamingState,
};

fn store() -> RuleStore {
    RuleStore::default()
}

fn naming() -> NamingState {
    NamingState::new()
}

fn default_rule_id(store: &RuleStore, type_id: i64) -> Option<String> {
    for entry in store.load_rules() {
        if entry.type_ == type_id && entry.default {
            return Some(entry.id);
        }
    }
    None
}

fn make_task(attribute: i64, title: &str) -> database::info::TaskInfo {
    let mut task_info = database::info::TaskInfo::default();
    task_info.Episode.attribute = attribute;
    task_info.Episode.leaf_title = title.to_string();
    task_info
}

fn freeze(task_info: &mut database::info::TaskInfo, options: Option<&NamingOptions>) {
    freeze_naming_rule(task_info, options, &store(), &naming());
}

fn update_file_name(task_info: &mut database::info::TaskInfo) {
    update_file_name_info(task_info, &store()).expect("naming succeeds");
}

// class TestDefaults:

#[test]
fn new_task_has_no_snapshot() {
    let task_info = database::info::TaskInfo::default();

    assert!(task_info.Naming.rule.is_none());
    assert!(task_info.Naming.rule_id.is_none());
    assert!(task_info.Naming.type_id.is_none());
}

#[test]
fn legacy_record_without_naming_section() {
    // 旧版本创建的任务，记录里根本没有 Naming 这一节
    let restored = database::info::TaskInfo::from_json(
        r#"{"Basic": {"task_id": "x", "cover_id": "", "show_title": "", "created_time": 0, "completed_time": 0}}"#,
    )
    .unwrap();

    assert!(restored.Naming.rule.is_none());
}

#[test]
fn round_trip_through_json() {
    // task.db 存的是 to_json() 的整段 JSON，快照必须能原样往返
    let mut task_info = make_task(attribute::VIDEO | attribute::NORMAL, "标题");
    freeze(&mut task_info, None);

    let restored = database::info::TaskInfo::from_json(&task_info.to_json()).unwrap();

    assert_eq!(restored.Naming.rule, task_info.Naming.rule);
    assert_eq!(restored.Naming.type_id, task_info.Naming.type_id);
}

// class TestFreezing:

#[test]
fn falls_back_to_the_type_default() {
    let mut task_info = make_task(attribute::VIDEO | attribute::NORMAL, "标题");
    freeze(&mut task_info, None);

    assert_eq!(task_info.Naming.type_id, Some(convention_type::NORMAL));
    assert!(task_info.Naming.rule.is_some());
}

#[test]
fn uses_the_selected_rule() {
    let rule_id = default_rule_id(&store(), convention_type::SPACE).unwrap();

    let mut task_info = make_task(attribute::SPACE | attribute::VIDEO | attribute::NORMAL, "标题");
    let mut map = HashMap::new();
    map.insert(convention_type::SPACE, rule_id.clone());
    let options = NamingOptions { naming_rule_ids: map };
    freeze(&mut task_info, Some(&options));

    assert_eq!(task_info.Naming.rule_id.as_deref(), Some(rule_id.as_str()));
}

#[test]
fn options_override_global_state() {
    // runtime.naming.target_rule_ids 被污染成不存在的规则时，空 options 必须覆盖
    let mut naming_state = naming();
    naming_state.target_rule_ids.insert(convention_type::NORMAL, "不存在的规则".to_string());

    let mut task_info = make_task(attribute::VIDEO | attribute::NORMAL, "标题");
    let options = NamingOptions::default();
    freeze_naming_rule(&mut task_info, Some(&options), &store(), &naming_state);

    // 查不到指定规则时回落该类型的默认规则，而不是抬手用全局状态
    assert!(task_info.Naming.rule.is_some());
}

// class TestRaceCondition:

#[test]
fn queued_task_is_not_rewritten_by_a_later_choice() {
    // 本文件的核心用例：排队中的任务不受后续选择影响
    let mut task_info = make_task(attribute::VIDEO | attribute::NORMAL, "视频标题");
    freeze(&mut task_info, None);
    update_file_name(&mut task_info);

    let frozen_name = task_info.File.name.clone();
    let frozen_folder = task_info.File.folder.clone();

    // 用户接着解析了别的链接，并选了另一条规则
    let mut naming_state = naming();
    naming_state.target_rule_ids.insert(convention_type::NORMAL, "另一条规则".to_string());

    // 下载开始，二次格式化只读固化下来的规则
    freeze_naming_rule(&mut task_info, None, &store(), &naming_state);
    update_file_name(&mut task_info);

    assert_eq!(task_info.File.name, frozen_name);
    assert_eq!(task_info.File.folder, frozen_folder);
}

// class TestHeterogeneousBatch:

#[test]
fn each_item_uses_its_own_type_rule() {
    // 界面上只为收藏夹指定了规则，剧集条目必须落到剧集自己的默认规则
    let favorite_rule_id = default_rule_id(&store(), convention_type::FAVORITE).unwrap();
    let mut map = HashMap::new();
    map.insert(convention_type::FAVORITE, favorite_rule_id);
    let options = NamingOptions { naming_rule_ids: map };

    let mut favorite = make_task(attribute::FAVLIST | attribute::VIDEO | attribute::NORMAL, "标题");
    let mut bangumi =
        make_task(attribute::FAVLIST | attribute::BANGUMI | attribute::NEED_PARSE, "标题");

    freeze(&mut favorite, Some(&options));
    freeze(&mut bangumi, Some(&options));

    assert_eq!(favorite.Naming.type_id, Some(convention_type::FAVORITE));
    assert_eq!(bangumi.Naming.type_id, Some(convention_type::BANGUMI));

    assert!(bangumi.Naming.rule_id.is_none());
    assert_ne!(bangumi.Naming.rule, favorite.Naming.rule);
}

// class TestEpisodeTitleMerge:

#[test]
fn entry_label_survives_a_multi_part_reparse() {
    // 来源解析器只写入口标签；稿件标题由二次解析的 related_titles 给出
    let episode_id = EpisodeData::add_episode();

    let mut existing = EpisodeData::get_episode_data(&episode_id);
    existing.insert("source_title".into(), "稍后再看".into());
    EpisodeData::set_episode_data(&episode_id, existing);

    let mut episode_info = serde_json::Map::new();
    episode_info.insert("episode_id".into(), episode_id.clone().into());
    episode_info.insert("title".into(), "04 アルカテイル".into());
    let mut related = serde_json::Map::new();
    related.insert(
        "parent_title".into(),
        "【KEY社20周年音乐专辑】Key BEST SELECTION".into(),
    );
    episode_info.insert("related_titles".into(), related.into());

    let merged = update_episode_info(&mut episode_info, &1.into());

    assert_eq!(merged["source_title"], "稍后再看");
    assert_eq!(merged["parent_title"], "【KEY社20周年音乐专辑】Key BEST SELECTION");
    assert_eq!(merged["number"], 1);

    EpisodeData::remove(&episode_id);
}

// class TestFailureHandling:

#[test]
fn unrenderable_rule_raises_instead_of_vanishing() {
    // 以前条目会静默消失，用户只看到下载数量对不上
    let mut task_info = make_task(attribute::VIDEO | attribute::NORMAL, "标题");
    task_info.Naming.rule = Some("{根本不存在的变量}".to_string());

    assert!(update_file_name_info(&mut task_info, &store()).is_err());
}

#[test]
fn convention_type_ids_match_enum() {
    // ids pinned by the Python enum; the flat type map depends on them
    assert_eq!(convention_type::NORMAL, ConventionType::Normal.id());
    assert_eq!(convention_type::SPACE, 50);
}
