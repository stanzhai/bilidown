//! Rule template compiler (Python `util/format/rule_template.py`).
//!
//! `<...>` optional segments on top of `{variable}` fields. Text nodes keep
//! their fields verbatim; rendering runs one blank-tracking vformat pass per
//! node per level so `is_blank` sees the raw values (`{p:02d}` renders `0`
//! as "00" — blankness must be decided before formatting).

use std::collections::HashMap;

use crate::format::{vformat, FormatError, Value};

/// Nesting depth cap for optional segments (guard against stack overflow on
/// hand-edited config strings).
pub const MAX_DEPTH: usize = 8;

/// Syntax error carrying the offending position for the editor's cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleSyntaxError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for RuleSyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// Plain text; may still contain `{fields}`, handled by vformat.
    Text(String),
    /// A `<...>` optional segment.
    Optional(Vec<Node>),
}

/// Python `is_blank`: "the variable did not get a value". Bool is checked
/// before int (False is a legitimate value); strings blank when only spaces
/// and dots remain after stripping.
pub fn is_blank(value: &Value) -> bool {
    match value {
        Value::None_ => true,
        Value::Bool(_) => false,
        Value::Text(s) => s.trim_matches(|c| c == ' ' || c == '.').is_empty(),
        Value::Int(i) => *i == 0,
        Value::Float(f) => *f == 0.0,
        Value::DateTime(_) => false,
    }
}

/// State gathered while one level's text nodes render.
#[derive(Default)]
struct LevelState {
    blank_hit: bool,
    field_count: usize,
}

/// `_scan_field`: from the `{` at `start`, return the index just past the
/// matching `}`. Doubled `{{` is a literal escape. Unclosed: strict raises,
/// lenient treats `{` as a literal (vformat reports it at render time).
fn scan_field(rule: &[char], start: usize, strict: bool) -> Result<usize, RuleSyntaxError> {
    if rule.get(start + 1) == Some(&'{') {
        return Ok(start + 2);
    }

    let mut depth = 0i32;
    let mut index = start;

    while index < rule.len() {
        match rule[index] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index + 1);
                }
            }
            _ => {}
        }
        index += 1;
    }

    if strict {
        return Err(RuleSyntaxError {
            message: "'{' 没有配对的 '}'".to_string(),
            position: start,
        });
    }

    Ok(start + 1)
}

/// `_parse_nodes`: parse one level; returns `(nodes, index)` where index
/// points at the closing `>` (or end of string).
fn parse_nodes(
    rule: &[char],
    pos: usize,
    depth: usize,
    strict: bool,
) -> Result<(Vec<Node>, usize), RuleSyntaxError> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut buffer = String::new();

    fn flush(nodes: &mut Vec<Node>, buffer: &mut String) {
        if !buffer.is_empty() {
            nodes.push(Node::Text(std::mem::take(buffer)));
        }
    }

    let mut index = pos;

    while index < rule.len() {
        match rule[index] {
            '{' => {
                let end = scan_field(rule, index, strict)?;
                buffer.extend(&rule[index..end]);
                index = end;
            }
            '<' => {
                if depth + 1 > MAX_DEPTH {
                    if strict {
                        return Err(RuleSyntaxError {
                            message: "可选段嵌套层数过多".to_string(),
                            position: index,
                        });
                    }
                    index += 1;
                    continue;
                }

                flush(&mut nodes, &mut buffer);

                let (children, end) = parse_nodes(rule, index + 1, depth + 1, strict)?;

                if end >= rule.len() || rule[end] != '>' {
                    if strict {
                        return Err(RuleSyntaxError {
                            message: "'<' 没有配对的 '>'".to_string(),
                            position: index,
                        });
                    }
                    // Lenient: splice the children in without the wrapper.
                    nodes.extend(children);
                    index = end;
                    continue;
                }

                nodes.push(Node::Optional(children));
                index = end + 1;
            }
            '>' => {
                if depth == 0 {
                    if strict {
                        return Err(RuleSyntaxError {
                            message: "'>' 没有配对的 '<'".to_string(),
                            position: index,
                        });
                    }
                    // Stray `>` is dropped, not kept: kept, it produces a
                    // filename Windows can't open.
                    index += 1;
                    continue;
                }

                flush(&mut nodes, &mut buffer);
                return Ok((nodes, index));
            }
            c => {
                buffer.push(c);
                index += 1;
            }
        }
    }

    flush(&mut nodes, &mut buffer);
    Ok((nodes, rule.len()))
}

/// A compiled naming rule, reusable across renders.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleTemplate {
    pub rule: String,
    pub nodes: Vec<Node>,
}

/// Render failure: unknown variable (KeyError in Python) or format syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderError {
    UnknownVariable(String),
    Syntax(String),
}

impl From<FormatError> for RenderError {
    fn from(e: FormatError) -> Self {
        match e {
            FormatError::UnknownVariable(k) => RenderError::UnknownVariable(k),
            FormatError::Syntax(s) => RenderError::Syntax(s),
        }
    }
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::UnknownVariable(k) => write!(f, "'{k}'"),
            RenderError::Syntax(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl RuleTemplate {
    pub fn render(&self, data: &HashMap<String, Value>) -> Result<String, RenderError> {
        Ok(self.render_nodes(&self.nodes, data)?.0)
    }

    /// Returns `(text, blank_hit, field_count, nested_total, nested_kept)`.
    ///
    /// Text nodes render **before** any drop decision: a mistyped variable
    /// inside a segment that would be dropped must still fail the rule.
    fn render_nodes(
        &self,
        nodes: &[Node],
        data: &HashMap<String, Value>,
    ) -> Result<(String, bool, usize, usize, usize), RenderError> {
        let mut state = LevelState::default();
        let mut pieces: Vec<String> = Vec::new();
        let mut nested_total = 0usize;
        let mut nested_kept = 0usize;

        for node in nodes {
            match node {
                Node::Text(text) => {
                    pieces.push(vformat(text, data)?);
                    track_fields(text, data, &mut state)?;
                }
                Node::Optional(children) => {
                    nested_total += 1;
                    let rendered = self.render_optional(children, data)?;
                    if !rendered.is_empty() {
                        nested_kept += 1;
                    }
                    pieces.push(rendered);
                }
            }
        }

        Ok((
            pieces.concat(),
            state.blank_hit,
            state.field_count,
            nested_total,
            nested_kept,
        ))
    }

    fn render_optional(
        &self,
        children: &[Node],
        data: &HashMap<String, Value>,
    ) -> Result<String, RenderError> {
        let (text, blank_hit, field_count, nested_total, nested_kept) =
            self.render_nodes(children, data)?;

        if blank_hit {
            // Any blank variable drops the whole segment including its
            // literal prefix/suffix ("any", not "all": <S{}E{}> missing an
            // episode number must not leave "S01E" behind).
            return Ok(String::new());
        }

        if field_count == 0 && nested_total > 0 && nested_kept == 0 {
            // No fields of its own and every nested child dropped: only
            // literals would remain, pointless.
            return Ok(String::new());
        }

        Ok(text)
    }

    /// Every variable name used by the rule (nested specs included), without
    /// rendering — for the editor's unknown-variable check.
    pub fn field_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();

        fn walk(nodes: &[Node], names: &mut Vec<String>) {
            for node in nodes {
                match node {
                    Node::Text(text) => collect_field_names(text, names),
                    Node::Optional(children) => walk(children, names),
                }
            }
        }

        walk(&self.nodes, &mut names);
        names.sort();
        names.dedup();
        names
    }

    /// Literal texts (everything outside `{}`): `<`/`>` appearing here are
    /// user-written junk, not delimiters.
    pub fn literal_texts(&self) -> Vec<String> {
        let mut texts = Vec::new();

        fn walk(nodes: &[Node], texts: &mut Vec<String>) {
            for node in nodes {
                match node {
                    Node::Text(text) => {
                        if let Ok(segments) = crate::format::parse(text) {
                            for seg in segments {
                                match seg {
                                    crate::format::Segment::Literal(lit) => {
                                        if !lit.is_empty() {
                                            texts.push(lit);
                                        }
                                    }
                                    crate::format::Segment::Field { literal, .. } => {
                                        if !literal.is_empty() {
                                            texts.push(literal);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Node::Optional(children) => walk(children, texts),
                }
            }
        }

        walk(&self.nodes, &mut texts);
        texts
    }

    /// Whether any optional segment contains no variable at all — it could
    /// never be dropped, so it's dead weight.
    pub fn has_empty_optional_segment(&self) -> bool {
        fn find(nodes: &[Node]) -> bool {
            for node in nodes {
                if let Node::Optional(children) = node {
                    if !has_field(children) {
                        return true;
                    }
                    if find(children) {
                        return true;
                    }
                }
            }
            false
        }

        find(&self.nodes)
    }
}

/// Counts fields and blank hits for a text node, mirroring the Python
/// formatter's get_value hooks.
fn track_fields(
    text: &str,
    data: &HashMap<String, Value>,
    state: &mut LevelState,
) -> Result<(), RenderError> {
    for seg in crate::format::parse(text).map_err(FormatError::Syntax)? {
        if let crate::format::Segment::Field { name, spec, .. } = seg {
            let value = data
                .get(&name)
                .ok_or_else(|| RenderError::UnknownVariable(name.clone()))?;
            state.field_count += 1;
            if is_blank(value) {
                state.blank_hit = true;
            }
            if !spec.is_empty() {
                track_fields(&spec, data, state)?;
            }
        }
    }
    Ok(())
}

/// Root name collection: `{a.b}` / `{a[0]}` yield `a` (rendering rejects
/// them anyway).
fn collect_field_names(text: &str, names: &mut Vec<String>) {
    if let Ok(segments) = crate::format::parse(text) {
        for seg in segments {
            if let crate::format::Segment::Field { name, spec, .. } = seg {
                if !name.is_empty() {
                    let root = name
                        .split('.')
                        .next()
                        .unwrap_or("")
                        .split('[')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    names.push(root);
                }
                if !spec.is_empty() {
                    collect_field_names(&spec, names);
                }
            }
        }
    }
}

fn has_field(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Text(text) => crate::format::parse(text)
            .map(|segs| {
                segs.iter()
                    .any(|s| matches!(s, crate::format::Segment::Field { .. }))
            })
            .unwrap_or(false),
        Node::Optional(_) => false,
    })
}

/// Compile a rule template. `strict` raises `RuleSyntaxError` for the editor;
/// the default lenient mode survives malformed input so one stray delimiter
/// can't take down every task's filename.
pub fn compile_rule(rule: &str, strict: bool) -> Result<RuleTemplate, RuleSyntaxError> {
    let chars: Vec<char> = rule.chars().collect();
    let (nodes, _) = parse_nodes(&chars, 0, 0, strict)?;
    Ok(RuleTemplate {
        rule: rule.to_string(),
        nodes,
    })
}
