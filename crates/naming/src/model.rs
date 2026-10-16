//! Visual-editor rule model (Python `util/format/rule_model.py`).
//!
//! Bridge between raw rule strings and the editor's fragment tree. Two
//! invariants: parse(to_rule(model)) == model (round-trip lossless), and
//! anything the editor can't express returns [`UnsupportedRule`] with a
//! reason — never panics, never silently drops a piece.

use crate::format::{parse as format_parse, Segment};
use crate::template::{compile_rule, Node, RuleSyntaxError};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fragment {
    /// `None` → plain text fragment (content in `text`); else a variable
    /// with `prefix`/`suffix` literals that appear/disappear with it.
    pub variable: Option<String>,
    pub spec: String,
    pub prefix: String,
    pub suffix: String,
    pub text: String,
}

impl Fragment {
    pub fn is_text(&self) -> bool {
        self.variable.is_none()
    }

    /// Only variables with prefix/suffix need the optional-segment syntax to
    /// bind the literals to them.
    pub fn is_optional(&self) -> bool {
        !self.is_text() && (!self.prefix.is_empty() || !self.suffix.is_empty())
    }
}

/// One directory level, or (as the last level) the file name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Level {
    pub fragments: Vec<Fragment>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleModel {
    pub levels: Vec<Level>,
}

/// The rule contains a structure the visual editor can't express; `reason`
/// tells the user where it got stuck.
#[derive(Debug, Clone, PartialEq)]
pub struct UnsupportedRule {
    pub reason: String,
}

fn escape(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

fn serialize_fragment(fragment: &Fragment) -> String {
    if fragment.is_text() {
        return escape(&fragment.text);
    }

    let field_text = match &fragment.variable {
        Some(v) => {
            if fragment.spec.is_empty() {
                format!("{{{v}}}")
            } else {
                format!("{{{v}:{}}}", fragment.spec)
            }
        }
        None => String::new(),
    };

    if !fragment.is_optional() {
        return field_text;
    }

    format!("<{}{}{}>", escape(&fragment.prefix), field_text, escape(&fragment.suffix))
}

impl RuleModel {
    pub fn to_rule(&self) -> String {
        self.levels
            .iter()
            .map(|level| {
                level
                    .fragments
                    .iter()
                    .map(serialize_fragment)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// Flattened parse item.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Text(String),
    Field { name: String, spec: String },
    Optional { prefix: String, name: String, spec: String, suffix: String },
}

/// Split a text node into literal/field items, raising `_Unsupported` shapes
/// for conversions, nested specs and unrecognizable field names.
fn iter_items(text: &str) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();

    for seg in format_parse(text).map_err(|_| String::from("规则中含有无法解析的格式串"))? {
        match seg {
            Segment::Literal(lit) => {
                if !lit.is_empty() {
                    items.push(Item::Text(lit));
                }
            }
            Segment::Field { literal, name, spec, conversion } => {
                if !literal.is_empty() {
                    items.push(Item::Text(literal));
                }

                if conversion.is_some() {
                    return Err("规则中使用了转换符（!r / !s），可视化编辑器无法表示".to_string());
                }

                if spec.contains('{') {
                    return Err("规则中使用了嵌套格式串，可视化编辑器无法表示".to_string());
                }

                if name.is_empty() || name.contains('.') || name.contains('[') || name.chars().all(|c| c.is_ascii_digit()) {
                    return Err("规则中含有无法识别的变量写法".to_string());
                }

                items.push(Item::Field { name, spec });
            }
        }
    }

    Ok(items)
}

/// Flatten an optional segment to (prefix, variable, spec, suffix).
fn flatten_optional(node: &[Node]) -> Result<(String, String, String, String), String> {
    let mut items = Vec::new();

    for child in node {
        match child {
            Node::Optional(_) => {
                return Err("规则中含有嵌套的可选段，可视化编辑器无法表示".to_string())
            }
            Node::Text(text) => items.extend(iter_items(text)?),
        }
    }

    let fields: Vec<(usize, &Item)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, Item::Field { .. }))
        .collect();

    if fields.is_empty() {
        return Err("规则中有不含变量的可选段".to_string());
    }

    if fields.len() > 1 {
        return Err("规则中有一个可选段包含多个变量，可视化编辑器无法表示".to_string());
    }

    let (index, field) = fields[0];
    let name = match field {
        Item::Field { name, .. } => name.clone(),
        _ => unreachable!(),
    };
    let spec = match field {
        Item::Field { spec, .. } => spec.clone(),
        _ => unreachable!(),
    };

    let text_of = |item: &Item| match item {
        Item::Text(t) => t.clone(),
        _ => String::new(),
    };

    let prefix: String = items[..index].iter().map(text_of).collect::<Vec<_>>().concat();
    let suffix: String = items[index + 1..].iter().map(text_of).collect::<Vec<_>>().concat();

    Ok((prefix, name, spec, suffix))
}

/// Flatten the node tree to items, normalizing optional segments in place.
fn node_items(nodes: &[Node]) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();

    for node in nodes {
        match node {
            Node::Text(text) => items.extend(iter_items(text)?),
            Node::Optional(children) => {
                let (prefix, name, spec, suffix) = flatten_optional(children)?;

                if prefix.is_empty() && suffix == "/" {
                    // <{x}/> ≡ {x}/ — empty directory levels get dropped by
                    // path normalization anyway; normalize to a plain level
                    // so the editor doesn't need a special shape for it.
                    items.push(Item::Field { name, spec });
                    items.push(Item::Text("/".to_string()));
                    continue;
                }

                if prefix.contains('/') || suffix.contains('/') {
                    return Err("规则中有跨越目录层级的可选段，可视化编辑器无法表示".to_string());
                }

                items.push(Item::Optional { prefix, name, spec, suffix });
            }
        }
    }

    Ok(items)
}

fn split_levels(items: Vec<Item>) -> Vec<Vec<Item>> {
    let mut levels: Vec<Vec<Item>> = vec![Vec::new()];

    for item in items {
        match item {
            Item::Text(text) => {
                let parts: Vec<&str> = text.split('/').collect();
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        levels.push(Vec::new());
                    }
                    if !part.is_empty() {
                        levels.last_mut().unwrap().push(Item::Text(part.to_string()));
                    }
                }
            }
            other => levels.last_mut().unwrap().push(other),
        }
    }

    levels
}

fn build_level(items: Vec<Item>) -> Level {
    let mut fragments: Vec<Fragment> = Vec::new();

    for item in items {
        match item {
            Item::Text(text) => {
                // Adjacent text merges; otherwise a round-trip sprouts empty
                // shell fragments.
                if let Some(last) = fragments.last_mut() {
                    if last.is_text() {
                        last.text.push_str(&text);
                        continue;
                    }
                }
                fragments.push(Fragment { text, ..Default::default() });
            }
            Item::Field { name, spec } => {
                fragments.push(Fragment {
                    variable: Some(name),
                    spec,
                    ..Default::default()
                });
            }
            Item::Optional { prefix, name, spec, suffix } => {
                fragments.push(Fragment {
                    variable: Some(name),
                    spec,
                    prefix,
                    suffix,
                    text: String::new(),
                });
            }
        }
    }

    Level { fragments }
}

/// Parse a raw rule string into the structural model. Returns
/// [`UnsupportedRule`] (never an error, never silent data loss) when the
/// editor can't express the rule.
pub fn parse_rule(rule: &str) -> Result<RuleModel, UnsupportedRule> {
    let template = match compile_rule(rule, true) {
        Ok(t) => t,
        Err(RuleSyntaxError { message, .. }) => return Err(UnsupportedRule { reason: message }),
    };

    let items = match node_items(&template.nodes) {
        Ok(items) => items,
        Err(reason) => return Err(UnsupportedRule { reason }),
    };

    Ok(RuleModel {
        levels: split_levels(items).into_iter().map(build_level).collect(),
    })
}
