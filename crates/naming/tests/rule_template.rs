//! test_rule_template.py — optional-segment delimiter scanning, blank-value
//! semantics, failure contract, introspection, and the issue-461 regression;
//! ported verbatim.

use std::collections::HashMap;

use naming::format::Value;
use naming::template::{compile_rule, is_blank, RenderError, RuleSyntaxError};

fn d(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn render(rule: &str, data: &HashMap<String, Value>) -> Result<String, RenderError> {
    compile_rule(rule, false).unwrap().render(data)
}

// class TestDelimiterScanning:

/// `{t:>6}` 的 `>` 是对齐符不是可选段定界符
#[test]
fn alignment_spec_is_not_a_segment() {
    assert_eq!(render("{t:>6}", &d(&[("t", Value::Text("ab".into()))])).unwrap(), "    ab");
    assert_eq!(render("{t:<6}", &d(&[("t", Value::Text("ab".into()))])).unwrap(), "ab    ");
}

#[test]
fn strftime_spec_untouched() {
    let dt = chrono::NaiveDate::from_ymd_opt(2026, 3, 7).unwrap().and_hms_opt(0, 0, 0).unwrap();
    assert_eq!(
        render("{d:%Y-%m-%d}", &d(&[("d", Value::DateTime(dt))])).unwrap(),
        "2026-03-07"
    );
}

#[test]
fn nested_format_spec() {
    assert_eq!(
        render("{t:{w}}", &d(&[("t", Value::Text("ab".into())), ("w", Value::Text(">6".into()))])).unwrap(),
        "    ab"
    );
}

#[test]
fn doubled_braces_are_literal() {
    assert_eq!(
        render("{{{t}}}", &d(&[("t", Value::Text("x".into()))])).unwrap(),
        "{x}"
    );
}

// class TestSyntaxErrors:

#[test]
fn strict_reports_position() {
    for (rule, position) in [("<{a}", 0), ("{a}>", 3), ("<{a}>>", 5)] {
        let err = compile_rule(rule, true).unwrap_err();
        assert_eq!(err.position, position, "{rule}");
    }
}

#[test]
fn lenient_mode_survives() {
    // 手改过配置文件的用户不该因为一个游离尖括号就让所有任务取不到文件名。
    for rule in ["<{a}", "{a}>"] {
        assert_eq!(
            render(rule, &d(&[("a", Value::Text("X".into()))])).unwrap(),
            "X",
            "{rule}"
        );
    }
}

#[test]
fn depth_limit() {
    assert!(matches!(
        compile_rule(&format!("{}{{a}}{}", "<".repeat(9), ">".repeat(9)), true),
        Err(RuleSyntaxError { .. })
    ));
}

// class TestIsBlank:

#[test]
fn blank_values() {
    for v in [
        Value::None_,
        Value::Text(String::new()),
        Value::Text("   ".into()),
        Value::Text("...".into()),
        Value::Text(" . ".into()),
        Value::Int(0),
        Value::Float(0.0),
    ] {
        assert!(is_blank(&v), "{v:?}");
    }
}

#[test]
fn not_blank_values() {
    // False 是 int 的子类但它是一个正经取值；"0" 是有内容的字符串
    for v in [
        Value::Text("0".into()),
        Value::Bool(false),
        Value::Bool(true),
        Value::Int(1),
        Value::Int(-1),
        Value::Float(0.5),
        Value::DateTime(chrono::NaiveDateTime::default()),
    ] {
        assert!(!is_blank(&v), "{v:?}");
    }
}

// class TestOptionalSegment:

#[test]
fn part_number_zero_drops_the_whole_segment() {
    // 单P视频的 p 是 0，整段必须消失，而不是渲染成 P00-
    assert_eq!(
        render("<P{p:02d}->{leaf_title}", &d(&[
            ("p", Value::Int(0)),
            ("leaf_title", Value::Text("标题".into())),
        ])).unwrap(),
        "标题"
    );
}

#[test]
fn part_number_present_keeps_the_segment() {
    assert_eq!(
        render("<P{p:02d}->{leaf_title}", &d(&[
            ("p", Value::Int(1)),
            ("leaf_title", Value::Text("标题".into())),
        ])).unwrap(),
        "P01-标题"
    );
}

#[test]
fn any_semantics() {
    // 取「任一为空即丢弃」而非「全部为空」，否则缺集号时会产出 S01E
    assert_eq!(
        render("<S{a:02d}E{b:02d}>", &d(&[("a", Value::Int(1)), ("b", Value::Int(0))])).unwrap(),
        ""
    );
    assert_eq!(
        render("<S{a:02d}E{b:02d}>", &d(&[("a", Value::Int(1)), ("b", Value::Int(8))])).unwrap(),
        "S01E08"
    );
}

#[test]
fn literal_prefix_and_suffix_go_with_it() {
    assert_eq!(render("【<{a}>】", &d(&[("a", Value::Text(String::new()))])).unwrap(), "【】");
    assert_eq!(render("<【{a}】>", &d(&[("a", Value::Text(String::new()))])).unwrap(), "");
}

#[test]
fn nested_segments() {
    let template = "<{a}/<{b}>>";
    assert_eq!(render(template, &d(&[("a", vtext("A")), ("b", vtext("B"))])).unwrap(), "A/B");
    assert_eq!(render(template, &d(&[("a", vtext("A")), ("b", vtext(""))])).unwrap(), "A/");
    assert_eq!(render(template, &d(&[("a", vtext("")), ("b", vtext("B"))])).unwrap(), "");
}

fn vtext(s: &str) -> Value {
    Value::Text(s.to_string())
}

#[test]
fn literal_only_outer_segment_collapses() {
    // 外层自己没有变量，嵌套子段又全被丢弃，只剩字面量，没有存在意义
    assert_eq!(render("x<-<{b}>>y", &d(&[("b", vtext(""))])).unwrap(), "xy");
    assert_eq!(render("x<-<{b}>>y", &d(&[("b", vtext("B"))])).unwrap(), "x-By");
}

// class TestFailureContract:

#[test]
fn unknown_variable_raises() {
    assert!(matches!(
        render("{titel}", &d(&[("title", vtext("x"))])),
        Err(RenderError::UnknownVariable(_))
    ));
}

#[test]
fn unknown_variable_inside_a_dropped_segment_still_raises() {
    // 错误优先于丢弃
    assert!(matches!(
        render("<{parent_title}{typo}>", &d(&[("parent_title", vtext(""))])),
        Err(RenderError::UnknownVariable(_))
    ));
}

#[test]
fn field_injection_rejected() {
    // 属性穿透与位置参数一律按未知变量处理
    for rule in ["{a.__class__}", "{a[0]}", "{}", "{0}"] {
        assert!(
            matches!(render(rule, &d(&[("a", vtext("x"))])), Err(RenderError::UnknownVariable(_))),
            "{rule}"
        );
    }
}

// class TestIntrospection:

#[test]
fn field_names() {
    let template = compile_rule("{a}/<P{b:02d}-{c}>", false).unwrap();

    let mut names = template.field_names();
    names.sort();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn field_names_include_nested_spec() {
    let mut names = compile_rule("{t:{w}}", false).unwrap().field_names();
    names.sort();
    assert_eq!(names, vec!["t", "w"]);
}

#[test]
fn literal_texts_exclude_delimiters() {
    assert_eq!(
        compile_rule("【{a}】/<P{b}->", false).unwrap().literal_texts(),
        vec!["【", "】/", "P", "-"]
    );
}

#[test]
fn empty_optional_segment_detected() {
    assert!(compile_rule("<abc>{a}", false).unwrap().has_empty_optional_segment());
    assert!(!compile_rule("<P{b}>", false).unwrap().has_empty_optional_segment());
    assert!(compile_rule("<{a}<abc>>", false).unwrap().has_empty_optional_segment());
}

// class TestIssue461:

const ISSUE461_RULE: &str = "{space_owner}/{parent_title}/<P{p:02d}->{leaf_title}";

#[test]
fn issue461_single_part() {
    let result = render(ISSUE461_RULE, &d(&[
        ("space_owner", vtext("UP主")),
        ("parent_title", vtext("")),
        ("p", Value::Int(0)),
        ("leaf_title", vtext("单P标题")),
    ])).unwrap();

    // 空的目录层留下一个空路径段，由 normalize_path 收敛
    assert_eq!(result, "UP主//单P标题");
}

#[test]
fn issue461_multi_part() {
    let result = render(ISSUE461_RULE, &d(&[
        ("space_owner", vtext("UP主")),
        ("parent_title", vtext("分P稿件")),
        ("p", Value::Int(1)),
        ("leaf_title", vtext("第一段")),
    ])).unwrap();

    assert_eq!(result, "UP主/分P稿件/P01-第一段");
}
