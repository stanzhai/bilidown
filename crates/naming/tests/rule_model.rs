//! test_rule_model.py — visual-editor ↔ raw rule round-trip invariants;
//! ported verbatim.

use naming::model::{parse_rule, Fragment, Level, RuleModel, UnsupportedRule};
use naming::naming_rules::DEFAULT_RULES;
use naming::template::compile_rule;

// class TestRoundTrip:

#[test]
fn parse_serialize_parse() {
    for rule in [
        "{leaf_title}",
        "{parent_title}/P{p}-{leaf_title}",
        "{a}/<P{p:02d}->{b}",
        "{pub_time:%Y-%m}/{leaf_title}",
        "{uploader} - {leaf_title}",
        "【{a}】{b}",
        "{{{a}}}",
    ] {
        let model = parse_rule(rule).unwrap_or_else(|e| panic!("{rule}: {e:?}"));

        assert_eq!(model.to_rule(), rule, "{rule}");
        let reparsed = parse_rule(&model.to_rule()).expect("reparse");
        assert_eq!(reparsed, model, "{rule}");
    }
}

#[test]
fn default_rules_are_all_representable() {
    // 内置规则必须全都能在可视化编辑器里打开
    for (id, name, type_, rule) in DEFAULT_RULES {
        let _ = (id, type_);
        let model = parse_rule(rule);
        assert!(model.is_ok(), "{name}: {rule}");
    }
}

#[test]
fn serialized_output_is_always_valid() {
    let model = RuleModel {
        levels: vec![
            Level { fragments: vec![Fragment { variable: Some("a".into()), spec: String::new(), prefix: String::new(), suffix: String::new(), text: String::new() }] },
            Level {
                fragments: vec![
                    Fragment { variable: Some("p".into()), spec: "02d".into(), prefix: "P".into(), suffix: "-".into(), text: String::new() },
                    Fragment { variable: None, spec: String::new(), prefix: String::new(), suffix: String::new(), text: "【x】".into() },
                    Fragment { variable: Some("b".into()), spec: String::new(), prefix: String::new(), suffix: String::new(), text: String::new() },
                ],
            },
        ],
    };
    // to_rule() 的输出必须恒能被严格模式接受
    compile_rule(&model.to_rule(), true).expect("valid serialized rule");
}

// class TestNormalization:

#[test]
fn optional_directory_level_is_flattened() {
    // 空的目录层本来就会被 normalize_path 丢掉，<{b}/> 与 {b}/ 等价
    let model = parse_rule("{a}/<{b}/>{c}").unwrap();

    assert_eq!(model.to_rule(), "{a}/{b}/{c}");
    assert_eq!(model.levels.len(), 3);
}

#[test]
fn no_optional_segment_spans_a_separator() {
    assert!(!parse_rule("{a}/<{b}/>{c}").unwrap().to_rule().contains("/>"));
}

// class TestStructure:

#[test]
fn levels_split_on_separator() {
    let model = parse_rule("{a}/{b}/{c}").unwrap();

    let counts: Vec<usize> = model.levels.iter().map(|l| l.fragments.len()).collect();
    assert_eq!(counts, vec![1, 1, 1]);
}

#[test]
fn prefix_and_suffix_captured() {
    let fragment = &parse_rule("<P{p:02d}->{t}").unwrap().levels[0].fragments[0];

    assert_eq!(
        (&fragment.prefix, fragment.variable.as_deref(), &fragment.spec, &fragment.suffix),
        (&"P".to_string(), Some("p"), &"02d".to_string(), &"-".to_string())
    );
    assert!(fragment.is_optional());
}

#[test]
fn plain_field_is_not_optional() {
    let fragment = &parse_rule("{t}").unwrap().levels[0].fragments[0];

    assert!(!fragment.is_optional());
    assert!(!fragment.is_text());
}

#[test]
fn adjacent_text_is_merged() {
    // 不合并的话往返一次就会多出几个空壳片段
    let level = &parse_rule("a{x}bc").unwrap().levels[0];

    let texts: Vec<&str> = level
        .fragments
        .iter()
        .filter(|f| f.is_text())
        .filter(|f| !f.text.is_empty())
        .map(|f| f.text.as_str())
        .collect();
    assert_eq!(texts, vec!["a", "bc"]);
}

// class TestUnsupported:

#[test]
fn returns_reason_instead_of_raising() {
    for rule in [
        "<{a}-{b}>",   // 段内多个变量
        "<{a}<{b}>>",  // 嵌套可选段
        "<abc>",       // 不含变量的可选段
        "{a!r}",       // 转换符
        "{a:{w}}",     // 嵌套格式串
        "<x{a}/y>",    // 跨目录层级的可选段
        "<{a}",        // 语法错误
        "{a.b}",       // 属性穿透
    ] {
        let result = parse_rule(rule);

        assert!(matches!(result, Err(UnsupportedRule { .. })), "{rule}");
        assert!(!result.unwrap_err().reason.is_empty(), "{rule}");
    }
}
