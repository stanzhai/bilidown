//! test_naming_convention.py — sample variable data must match the runtime
//! key space exactly, and the editor-facing declarations must agree with the
//! formatter's type mapping; ported verbatim.

use std::collections::HashMap;

use naming::conventions::{
    all_variables, supported_shapes, variable_type, ConventionType, SampleShape,
    VariableListFactory, VARIABLE_DESCRIPTION,
};
use naming::format::Value;
use naming::formatter::{attribute, convention_type, FileNameFormatter};
use naming::naming_rules::RuleStore;

fn factory() -> VariableListFactory {
    VariableListFactory::new()
}

fn runtime_keys() -> HashMap<String, Value> {
    let store = RuleStore::default();
    let formatter = FileNameFormatter::new(HashMap::new(), &store);
    // variable_data() is what tests compare against; use
    // get_variable_data_from_task_info(TaskInfo::default()) for the key space.
    let _ = formatter;
    naming::formatter::get_variable_data_from_task_info(&Default::default())
}

// class TestKeySpace:

#[test]
fn key_space_matches_runtime() {
    let keys = runtime_keys();

    for type_ in ConventionType::ALL {
        for shape in SampleShape::ALL {
            let data = factory().build_variable_data(type_, shape);
            let names: std::collections::HashSet<&str> = data.keys().map(|s| s.as_str()).collect();
            let runtime: std::collections::HashSet<&str> =
                keys.keys().map(|s| s.as_str()).collect();
            assert_eq!(names, runtime, "{type_:?} {shape:?}");
        }
    }
}

#[test]
fn full_list_covers_the_whole_key_space() {
    let keys = runtime_keys();

    for type_ in ConventionType::ALL {
        let names: std::collections::HashSet<&str> = factory()
            .build(type_, true)
            .iter()
            .map(|e| e.name)
            .collect();
        let runtime: std::collections::HashSet<&str> = keys.keys().map(|s| s.as_str()).collect();
        assert_eq!(names, runtime, "{type_:?}");
    }
}

// class TestVariableList:

#[test]
fn source_types_expose_structural_variables() {
    // 来源类规则也要能用分P、合集的变量
    for type_ in [
        ConventionType::Space,
        ConventionType::Favorite,
        ConventionType::History,
        ConventionType::WatchLater,
    ] {
        let names: std::collections::HashSet<&str> = factory()
            .build(type_, true)
            .iter()
            .map(|e| e.name)
            .collect();

        for structural in ["parent_title", "p", "collection_title", "section_title"] {
            assert!(names.contains(structural), "{type_:?} {structural}");
        }
    }
}

#[test]
fn primary_list_is_unchanged() {
    // full = false 必须原样给出类型专属清单，顺序也不能被打乱
    let primary = factory().build(ConventionType::Part, false);

    let names: Vec<&str> = primary.iter().map(|e| e.name).collect();
    assert_eq!(
        names,
        vec![
            "pub_time",
            "pub_ts",
            "create_time",
            "create_ts",
            "number",
            "uploader",
            "uploader_uid",
            "video_quality",
            "audio_quality",
            "video_codec",
            "parent_title",
            "p",
            "leaf_title",
            "aid",
            "bvid",
            "cid",
        ]
    );
}

#[test]
fn groups_are_tagged() {
    let entries = factory().build(ConventionType::Space, true);
    let groups: HashMap<&str, &str> = entries.iter().map(|e| (e.name, e.group)).collect();

    assert_eq!(groups["space_owner"], "PRIMARY");
    assert_eq!(groups["p"], "MORE");
}

#[test]
fn every_entry_carries_a_type() {
    for type_ in ConventionType::ALL {
        for entry in factory().build(type_, true) {
            // 可视化编辑器据此决定格式项的形态；VariableType is not Option
            let _ = entry.type_;
        }
    }
}

#[test]
fn number_like_variables_typed_as_number() {
    let types: HashMap<&str, _> = factory()
        .build(ConventionType::Bangumi, true)
        .iter()
        .map(|e| (e.name, e.type_))
        .collect();

    assert_eq!(
        types["episode_number"],
        naming::conventions::VariableType::Number
    );
    assert_eq!(
        types["pub_time"],
        naming::conventions::VariableType::Datetime
    );
    assert_eq!(types["leaf_title"], naming::conventions::VariableType::Text);
    assert_eq!(
        variable_type("episode_number"),
        naming::conventions::VariableType::Number
    );
}

#[test]
fn descriptions_are_translatable() {
    let described: std::collections::HashSet<&str> =
        VARIABLE_DESCRIPTION.iter().map(|(k, _)| *k).collect();

    for type_ in ConventionType::ALL {
        for entry in factory().build(type_, true) {
            assert!(described.contains(entry.description), "{}", entry.name);
        }
    }
}

// class TestSampleData:

#[test]
fn shapes_are_declared_for_every_type() {
    let mut declared = std::collections::HashSet::new();
    for type_ in ConventionType::ALL {
        declared.insert(type_);
        assert!(!supported_shapes(type_).is_empty(), "{type_:?}");
    }
    assert_eq!(declared.len(), ConventionType::ALL.len());
}

#[test]
fn single_shape_clears_structural_values() {
    let data = factory().build_variable_data(ConventionType::Space, SampleShape::Single);

    assert_eq!(data["parent_title"], Value::Text(String::new()));
    assert_eq!(data["p"], Value::Int(0));
}

#[test]
fn multi_shape_fills_structural_values() {
    let data = factory().build_variable_data(ConventionType::Space, SampleShape::Multi);

    assert!(matches!(&data["parent_title"], Value::Text(s) if !s.is_empty()));
    assert_eq!(data["p"], Value::Int(4));
}

#[test]
fn parent_title_is_structural_only() {
    // {parent_title} 从 2.20.0 起只表示稿件标题；入口标签已挪进 {source_title}
    let single = factory().build_variable_data(ConventionType::History, SampleShape::Single);
    let multi = factory().build_variable_data(ConventionType::History, SampleShape::Multi);

    assert_eq!(single["parent_title"], Value::Text(String::new()));
    assert!(matches!(&multi["parent_title"], Value::Text(s) if !s.is_empty()));
}

#[test]
fn source_title_survives_every_shape() {
    // 入口标签与条目形态无关
    for shape in SampleShape::ALL {
        let data = factory().build_variable_data(ConventionType::History, shape);
        assert_eq!(
            data["source_title"],
            Value::Text("历史记录".into()),
            "{shape:?}"
        );
    }
}

#[test]
fn source_title_is_empty_outside_the_source_types() {
    for type_ in [
        ConventionType::Normal,
        ConventionType::Favorite,
        ConventionType::Collection,
    ] {
        let data = factory().build_variable_data(type_, SampleShape::Single);
        assert_eq!(
            data["source_title"],
            Value::Text(String::new()),
            "{type_:?}"
        );
    }
}

#[test]
fn numeric_sentinels_survive_number_formatting() {
    // 哨兵若是空串，{aid:>12} 这类写法会在预览里抛错
    let store = RuleStore::default();
    let formatter = FileNameFormatter::new(
        factory().build_variable_data(ConventionType::Space, SampleShape::Single),
        &store,
    );

    assert!(formatter
        .format(None, 0, "{aid:>12}/{season_number:02d}/{pub_time:%Y-%m}")
        .is_some());
}

#[test]
fn collection_shape_keeps_the_collection_title() {
    // 合集条目的 collection_title 是归属信息，不是形态信息
    let single = factory().build_variable_data(ConventionType::Collection, SampleShape::Single);

    assert!(matches!(&single["collection_title"], Value::Text(s) if !s.is_empty()));
    assert!(matches!(&single["leaf_title"], Value::Text(s) if !s.is_empty()));
    assert_eq!(single["parent_title"], Value::Text(String::new()));
    assert_eq!(single["p"], Value::Int(0));
    assert_eq!(single["section_title"], Value::Text(String::new()));
}

#[test]
fn collection_shapes_differ() {
    let single = factory().build_variable_data(ConventionType::Collection, SampleShape::Single);
    let full = factory().build_variable_data(ConventionType::Collection, SampleShape::Collection);

    assert_eq!(single["collection_title"], full["collection_title"]);
    assert_ne!(single["leaf_title"], full["leaf_title"]);
}

#[test]
fn datetime_variables_are_datetime() {
    let data = factory().build_variable_data(ConventionType::Normal, SampleShape::Single);

    for name in ["pub_time", "create_time", "fav_time", "last_watched_time"] {
        assert!(matches!(data[name], Value::DateTime(_)), "{name}");
    }
}

// class TestMixedEntryTypes:

#[test]
fn declared_mixed_entries_match_the_type_mapping() {
    use naming::conventions::MIXED_ENTRY_TYPES;
    use naming::formatter::get_type_id_from_attribute;

    let source_bits: HashMap<ConventionType, i64> = [
        (ConventionType::Favorite, attribute::FAVLIST),
        (ConventionType::Space, attribute::SPACE),
        (ConventionType::History, attribute::HISTORY),
        (ConventionType::WatchLater, attribute::WATCH_LATER),
    ]
    .into_iter()
    .collect();

    let kind_bits: HashMap<ConventionType, i64> = [
        (ConventionType::Bangumi, attribute::BANGUMI),
        (ConventionType::Cheese, attribute::CHEESE),
    ]
    .into_iter()
    .collect();

    for (source, mixed) in MIXED_ENTRY_TYPES {
        for kind in *mixed {
            let attr = source_bits[source] | kind_bits[kind] | attribute::NEED_PARSE;
            let resolved = get_type_id_from_attribute(attr).expect("resolvable");

            assert_eq!(resolved, kind.id(), "{source:?} {kind:?}");
        }
    }
}

#[test]
fn only_multi_shape_types_declare_mixed_entries() {
    // 只有装着多种形态条目的类型才会同时装着投稿视频与影视、课程
    for (type_, _) in naming::conventions::MIXED_ENTRY_TYPES {
        assert!(supported_shapes(*type_).len() > 1, "{type_:?}");
    }
}

// class TestIssue461:

const ISSUE461_RULE: &str = "{space_owner}/{parent_title}/<P{p:02d}->{leaf_title}";

fn render_issue461(shape: SampleShape) -> Option<String> {
    let store = RuleStore::default();
    let formatter = FileNameFormatter::new(
        factory().build_variable_data(ConventionType::Space, shape),
        &store,
    );
    formatter.format(None, 0, ISSUE461_RULE)
}

#[test]
fn preview_no_longer_rejects_structural_variables() {
    assert!(render_issue461(SampleShape::Multi).is_some());
}

#[test]
fn single_and_multi_render_differently() {
    let single = render_issue461(SampleShape::Single).unwrap();
    let multi = render_issue461(SampleShape::Multi).unwrap();

    // 单P不该留下 P00- 这样的残缺前缀
    assert!(!single.contains("P0"), "{single}");
    assert!(multi.contains("P04-"), "{multi}");
}

#[test]
fn all_variables_key_space_is_constant() {
    // all_variables 的名字集合对所有类型都完整（编辑器降级清单的来源）
    let names: std::collections::HashSet<&str> = all_variables().iter().map(|e| e.name).collect();
    let keys = runtime_keys();
    let runtime: std::collections::HashSet<&str> = keys.keys().map(|s| s.as_str()).collect();
    assert_eq!(names, runtime);
    assert_eq!(convention_type::FAVORITE, 40);
}
