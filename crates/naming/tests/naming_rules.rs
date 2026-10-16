//! test_naming_rules.py — the rule store must hand out copies the caller can
//! mutate freely (the download-options dialog renames entries to show
//! translations); ported verbatim.
use naming::formatter;
use naming::conventions::ConventionType;
use naming::naming_rules::{display_name, RuleEntry, RuleStore};

// class TestCopyIsolation:

#[test]
fn load_rules_returns_a_new_list_each_time() {
    let store = RuleStore::default();
    assert_ne!(store.load_rules().first().map(|e| e.id.as_str()), None);
    // Rust: separate Vec allocations each call; equality by content still holds
    let first = store.load_rules();
    let second = store.load_rules();
    assert_eq!(first, second);
    assert!(!std::ptr::eq(first.as_ptr(), second.as_ptr()));
}

#[test]
fn load_rules_deep_copies_entries() {
    // 浅拷贝挡不住这一层：元素仍是配置里那个字典
    let store = RuleStore::default();
    let first = store.load_rules();
    let second = store.load_rules();

    let a = first[0].rule.as_str();
    let b = second[0].rule.as_str();
    assert!(!std::ptr::eq(a, b));
}

#[test]
fn mutating_loaded_rules_does_not_touch_config() {
    let store = RuleStore::default();
    let original = store.load_rules()[0].rule.clone();

    let mut rules = store.load_rules();
    rules[0].rule = "{leaf_title}_被改过了".to_string();

    assert_eq!(store.load_rules()[0].rule, original);
}

#[test]
fn mutating_default_rules_does_not_touch_default_value() {
    let store = RuleStore::default();
    let mut rules = store.load_default_rules();
    let original = rules[0].name.clone();

    rules[0].name = "被改过了".to_string();

    assert_eq!(store.load_default_rules()[0].name, original);
}

#[test]
fn rule_list_from_attribute_is_detached() {
    // 下载选项对话框拿到这份列表后会改 name 来显示译名，改动不得回流到配置
    let store = RuleStore::default();
    let mut rule_list = store.get_rule_list_from_attribute(formatter::attribute::SPACE);

    assert!(!rule_list.is_empty(), "个人空间应当至少有一条默认规则");

    rule_list[0].name = "被改过了".to_string();

    for entry in store.load_rules() {
        if entry.type_ == ConventionType::Space.id() {
            assert_ne!(entry.name, "被改过了");
        }
    }
}

// class TestDisplayName:

#[test]
fn builtin_key_is_translated() {
    let entry = RuleEntry {
        id: String::new(),
        name: "DEFAULT_FOR_NORMAL".to_string(),
        type_: 0,
        rule: String::new(),
        default: false,
    };

    // 只关心「不是翻译键本身」
    assert_ne!(display_name(&entry), "DEFAULT_FOR_NORMAL");
}

#[test]
fn custom_name_passes_through() {
    let entry = RuleEntry {
        id: String::new(),
        name: "我自己建的规则".to_string(),
        type_: 0,
        rule: String::new(),
        default: false,
    };

    assert_eq!(display_name(&entry), "我自己建的规则");
}

#[test]
fn does_not_write_back() {
    let entry = RuleEntry {
        id: String::new(),
        name: "DEFAULT_FOR_NORMAL".to_string(),
        type_: 0,
        rule: String::new(),
        default: false,
    };

    let _ = display_name(&entry);

    assert_eq!(entry.name, "DEFAULT_FOR_NORMAL");
}

// Rust store: save → load reflects the change (config-backed in Python)
#[test]
fn save_then_load_reflects_new_rules() {
    let mut store = RuleStore::default();
    let mut list = store.load_rules();
    list.push(RuleEntry {
        id: "custom-1".to_string(),
        name: "我自己建的规则".to_string(),
        type_: ConventionType::Normal.id(),
        rule: "{uploader} - {leaf_title}".to_string(),
        default: false,
    });
    store.save_rules(list);

    let loaded = store.load_rules();
    assert!(loaded.iter().any(|e| e.id == "custom-1"));
    assert_eq!(store.get_rule_by_id("custom-1").as_deref(), Some("{uploader} - {leaf_title}"));
}
