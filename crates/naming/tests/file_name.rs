//! test_file_name.py — rule rendering and path sanitization: server strings
//! become local paths here, so every case pins "no crash, no escape";
//! ported verbatim.

use naming::conventions::ConventionType;
use naming::format::Value;
use naming::formatter::{attribute, convention_type, FileNameFormatter};
use naming::naming_rules::RuleStore;

fn d(pairs: &[(&str, Value)]) -> std::collections::HashMap<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn t(s: &str) -> Value {
    Value::Text(s.to_string())
}

fn render(rule: &str, data: &[(&str, Value)]) -> Option<String> {
    let store = RuleStore::default();
    let formatter = FileNameFormatter::new(d(data), &store);
    formatter.format(None, 0, rule)
}

// class TestSanitize:

#[test]
fn plain_title_untouched() {
    assert_eq!(
        render("{title}", &[("title", t("正常标题"))]),
        Some("正常标题".into())
    );
}

#[test]
fn windows_reserved_chars_replaced() {
    // 这几个字符在 Windows 上不合法，在 POSIX 上 "/" 还会凭空多出一级目录
    for c in ['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
        assert_eq!(
            render("{title}", &[("title", t(&format!("a{c}b")))]),
            Some("a_b".into()),
            "{c:?}"
        );
    }
}

#[test]
fn control_chars_replaced() {
    for c in ['\0', '\x1f', '\n', '\t'] {
        assert_eq!(
            render("{title}", &[("title", t(&format!("a{c}b")))]),
            Some("a_b".into()),
            "{c:?}"
        );
    }
}

#[test]
fn non_string_values_pass_through() {
    // 序号、aid 等是整数，不应被净化逻辑破坏
    assert_eq!(render("{n}", &[("n", Value::Int(42))]), Some("42".into()));
}

// class TestPathTraversal:

/// result 是否含 ".." 路径段（`".." not in Path(result).parts`）
fn parts(result: &str) -> Vec<&str> {
    result.split(['/', '\\']).collect()
}

#[test]
fn dotdot_inside_variable() {
    // 变量里的分隔符会被替换成下划线，".." 因此无法独立成段
    let result = render("{title}", &[("title", t("../../etc/passwd"))]).unwrap();

    assert!(!parts(&result).contains(&".."));
    assert_eq!(result, "_.._etc_passwd");
}

#[test]
fn dotdot_as_rule_segment() {
    // 规则模板里写死的 ".." 段由 strip(" .") 清掉
    let result = render("{a}/{b}", &[("a", t("..")), ("b", t("evil"))]).unwrap();

    assert!(!parts(&result).contains(&".."));
    assert_eq!(result, "evil");
}

#[test]
fn leading_slash_does_not_produce_absolute_path() {
    let result = render("/{title}", &[("title", t("x"))]).unwrap();
    assert!(!result.starts_with('/'), "{result}");
}

#[test]
fn trailing_dots_and_spaces_stripped() {
    // Windows 上以点或空格结尾的目录名无法正常创建与删除
    assert_eq!(
        render("{a}/{b}", &[("a", t("dir. ")), ("b", t("file ."))]),
        Some("dir/file".into())
    );
}

#[test]
fn empty_render_falls_back() {
    // 渲染结果为空时不能返回空串
    assert_eq!(render("{a}", &[("a", t(""))]), Some("_".into()));
}

#[test]
fn everything_stripped_falls_back() {
    assert_eq!(render("{a}", &[("a", t("..."))]), Some("_".into()));
}

// class TestFailureHandling:

#[test]
fn missing_variable_returns_none() {
    assert_eq!(render("{missing}", &[("title", t("x"))]), None);
}

#[test]
fn empty_rule_without_type_id_returns_none() {
    // 既没有显式规则也没有 type_id：FALLBACK_RULE 引用的 leaf_title 在空
    // 数据里不存在，同样返回 None
    let store = RuleStore::default();
    let formatter = FileNameFormatter::new(d(&[]), &store);
    assert_eq!(formatter.format(None, 0, ""), None);
}

// class TestTypeMapping:

#[test]
fn attribute_maps_to_convention_type() {
    use naming::formatter::get_type_id_from_attribute;

    assert_eq!(
        get_type_id_from_attribute(attribute::FAVLIST),
        Some(convention_type::FAVORITE)
    );
    assert_eq!(
        get_type_id_from_attribute(attribute::SPACE),
        Some(convention_type::SPACE)
    );
    assert_eq!(
        get_type_id_from_attribute(attribute::BANGUMI),
        Some(convention_type::BANGUMI)
    );

    // 会员购商城课程并入课程，属性位还在但命名类型跟着并了过去
    assert_eq!(
        get_type_id_from_attribute(attribute::LESSON),
        Some(convention_type::CHEESE)
    );

    // 没有结构形态位也没有来源位的纯投稿视频按单个视频处理
    assert_eq!(
        get_type_id_from_attribute(attribute::VIDEO),
        Some(convention_type::NORMAL)
    );
    assert_eq!(
        get_type_id_from_attribute(attribute::VIDEO | attribute::PART),
        Some(convention_type::PART)
    );
}

#[test]
fn media_kind_outranks_the_source() {
    use naming::formatter::get_type_id_from_attribute;

    // 收藏夹、历史记录、稍后再看、个人空间里混着影视与课程条目；
    // 命名类型必须取媒体形态
    assert_eq!(
        get_type_id_from_attribute(attribute::FAVLIST | attribute::BANGUMI | attribute::NEED_PARSE),
        Some(convention_type::BANGUMI)
    );
    assert_eq!(
        get_type_id_from_attribute(
            attribute::WATCH_LATER | attribute::BANGUMI | attribute::NEED_PARSE
        ),
        Some(convention_type::BANGUMI)
    );
    assert_eq!(
        get_type_id_from_attribute(attribute::HISTORY | attribute::BANGUMI | attribute::NEED_PARSE),
        Some(convention_type::BANGUMI)
    );
    assert_eq!(
        get_type_id_from_attribute(attribute::HISTORY | attribute::CHEESE | attribute::NEED_PARSE),
        Some(convention_type::CHEESE)
    );
    assert_eq!(
        get_type_id_from_attribute(attribute::SPACE | attribute::CHEESE),
        Some(convention_type::CHEESE)
    );
}

#[test]
fn structure_bits_do_not_outrank_the_source() {
    use naming::formatter::get_type_id_from_attribute;

    // 来源类型要吞下列表里的普通视频、分P与合集
    for shape in [attribute::NORMAL, attribute::PART, attribute::COLLECTION] {
        assert_eq!(
            get_type_id_from_attribute(
                attribute::FAVLIST | attribute::VIDEO | shape | attribute::NEED_PARSE
            ),
            Some(convention_type::FAVORITE)
        );
    }
}

#[test]
fn collection_entries_stay_collection_after_reparse() {
    use naming::formatter::get_type_id_from_attribute;

    // 合集条目二次解析后补上 NORMAL/PART —— 归属没变，命名类型必须仍是 COLLECTION
    let resolved = attribute::COLLECTION_LIST | attribute::COLLECTION | attribute::VIDEO;

    for shape in [attribute::NORMAL, attribute::PART] {
        assert_eq!(
            get_type_id_from_attribute(resolved | shape),
            Some(convention_type::COLLECTION)
        );
    }
}

#[test]
fn unknown_attribute_returns_none() {
    use naming::formatter::get_type_id_from_attribute;

    assert_eq!(get_type_id_from_attribute(0), None);
    assert_eq!(ConventionType::ALL.len(), 12);
}

// class TestOptionalSegmentSafety:

#[test]
fn traversal_inside_an_optional_segment() {
    let result = render("<{title}>", &[("title", t("../../etc/passwd"))]).unwrap();

    assert!(!parts(&result).contains(&".."));
    assert_eq!(result, "_.._etc_passwd");
}

#[test]
fn separator_inside_an_optional_segment() {
    assert_eq!(
        render("<{a}/>{b}", &[("a", t("x/y")), ("b", t("n"))]),
        Some("x_y/n".into())
    );
}

#[test]
fn literal_dotdot_in_an_optional_segment_stripped() {
    assert_eq!(
        render("<{a}/../>{b}", &[("a", t("dir")), ("b", t("n"))]),
        Some("dir/n".into())
    );
}

#[test]
fn everything_dropped_falls_back() {
    assert_eq!(
        render("<{a}><{b}>", &[("a", t("")), ("b", Value::Int(0))]),
        Some("_".into())
    );
}

#[test]
fn unknown_variable_in_a_dropped_segment_still_fails() {
    // 错误优先于丢弃
    assert_eq!(
        render(
            "<{parent_title}{typo}>{title}",
            &[("parent_title", t("")), ("title", t("x"))]
        ),
        None
    );
}

// class TestIssue461EndToEnd:

const ISSUE461_RULE: &str = "{space_owner}/{parent_title}/<P{p:02d}->{leaf_title}";

#[test]
fn issue461_end_to_end_single_part() {
    let result = render(
        ISSUE461_RULE,
        &[
            ("space_owner", t("UP主")),
            ("parent_title", t("")),
            ("p", Value::Int(0)),
            ("leaf_title", t("单P标题")),
        ],
    )
    .unwrap();

    assert_eq!(result, "UP主/单P标题");
}

#[test]
fn issue461_end_to_end_multi_part() {
    let result = render(
        ISSUE461_RULE,
        &[
            ("space_owner", t("UP主")),
            ("parent_title", t("分P稿件")),
            ("p", Value::Int(1)),
            ("leaf_title", t("第一段")),
        ],
    )
    .unwrap();

    assert_eq!(result, "UP主/分P稿件/P01-第一段");
}
