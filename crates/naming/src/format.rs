//! Python `str.format` semantics for rule templates.
//!
//! The Python engine renders fields with `string.Formatter.vformat`, so a
//! faithful port needs the parts of the format-spec grammar the naming rules
//! actually exercise: alignment/fill, sign, `#`/`0`/`,` flags, width,
//! precision and presentation types, plus `!r`/`!s` conversions. Values are
//! the tagged [`Value`] enum the naming engine passes around (str, int,
//! float, bool, None, datetime).

use std::collections::HashMap;

use chrono::NaiveDateTime;

/// A renderable variable value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    None_,
    DateTime(NaiveDateTime),
}

impl Value {
    /// Python `str(value)` / `format(value, "")`.
    fn unformatted(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => python_float_repr(*f),
            Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Value::None_ => "None".to_string(),
            Value::DateTime(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    /// Python `repr(value)`.
    fn repr(&self) -> String {
        match self {
            Value::Text(s) => format!("'{}'", s),
            Value::None_ => "None".to_string(),
            Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => python_float_repr(*f),
            Value::DateTime(dt) => {
                format!("datetime.datetime({})", dt.format("%Y, %-m, %-d, %-H, %-M, %-S"))
            }
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_) | Value::Bool(_))
    }
}

/// `str(2.0)` is `"2.0"` in Python; Rust's `Display` gives `"2"`.
fn python_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("nan") {
        s
    } else {
        format!("{s}.0")
    }
}

/// One literal run and its following field (if any); a lone trailing literal
/// becomes a `Literal` segment. Mirrors `string.Formatter.parse` order.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Literal(String),
    Field {
        literal: String,
        /// Empty means positional `{}` / `{0}`.
        name: String,
        spec: String,
        conversion: Option<char>,
    },
}

/// Scans a `{...}` field starting at the `{`. Returns `(name, conversion,
/// spec, index_after_closing_brace)`.
fn scan_field(chars: &[char], open: usize) -> Result<(String, Option<char>, String, usize), String> {
    // Field name: up to the first `:` `!` or `}`.
    let mut i = open + 1;
    while i < chars.len() && !matches!(chars[i], ':' | '!' | '}') {
        i += 1;
    }
    let name: String = chars[open + 1..i].iter().collect();
    let mut conversion = None;

    if i < chars.len() && chars[i] == '!' {
        i += 1;
        if i >= chars.len() || matches!(chars[i], '}' | ':') {
            return Err("expected '}' before end of string".to_string());
        }
        conversion = Some(chars[i]);
        i += 1;
    }

    let spec_start = if i < chars.len() && chars[i] == ':' { i + 1 } else { i };
    let mut depth = 0usize;
    let mut j = spec_start;
    while j < chars.len() {
        match chars[j] {
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
        j += 1;
    }
    if j >= chars.len() {
        return Err("unmatched '{' in format string".to_string());
    }

    let spec: String = chars[spec_start..j].iter().collect();
    Ok((name, conversion, spec, j + 1))
}

/// Python `string.Formatter.parse`.
pub fn parse(template: &str) -> Result<Vec<Segment>, String> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = Vec::new();
    let mut literal = String::new();
    let mut index = 0;

    while index < chars.len() {
        match chars[index] {
            '{' => {
                if chars.get(index + 1) == Some(&'{') {
                    literal.push('{');
                    index += 2;
                    continue;
                }
                let (name, conversion, spec, end) = scan_field(&chars, index)?;
                out.push(Segment::Field {
                    literal: std::mem::take(&mut literal),
                    name,
                    spec,
                    conversion,
                });
                index = end;
            }
            '}' => {
                if chars.get(index + 1) == Some(&'}') {
                    literal.push('}');
                    index += 2;
                    continue;
                }
                return Err("Single '}' encountered in format string".to_string());
            }
            c => {
                literal.push(c);
                index += 1;
            }
        }
    }

    if !literal.is_empty() {
        out.push(Segment::Literal(literal));
    }

    Ok(out)
}

/// Errors surfaced by `vformat`.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatError {
    UnknownVariable(String),
    Syntax(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::UnknownVariable(k) => write!(f, "'{k}'"),
            FormatError::Syntax(s) => write!(f, "{s}"),
        }
    }
}

/// Field lookup with the naming engine's rejection rules: `{}`, positional
/// and `{a.b}` / `{a[0]}` accesses are all unknown variables here.
fn lookup<'a>(name: &str, data: &'a HashMap<String, Value>) -> Result<&'a Value, FormatError> {
    if name.is_empty() || name.contains('.') || name.contains('[') || name.chars().all(|c| c.is_ascii_digit()) {
        return Err(FormatError::UnknownVariable(name.to_string()));
    }
    data.get(name).ok_or_else(|| FormatError::UnknownVariable(name.to_string()))
}

/// Python `Formatter.vformat(template, (), data)`.
pub fn vformat(template: &str, data: &HashMap<String, Value>) -> Result<String, FormatError> {
    let mut out = String::new();

    for segment in parse(template).map_err(FormatError::Syntax)? {
        match segment {
            Segment::Literal(lit) => out.push_str(&lit),
            Segment::Field { literal, name, spec, conversion } => {
                out.push_str(&literal);
                let value = lookup(&name, data)?;
                let spec = if spec.is_empty() {
                    String::new()
                } else {
                    vformat(&spec, data)?
                };
                out.push_str(&format_value(value, conversion, &spec)?);
            }
        }
    }

    Ok(out)
}

/// Parsed format-spec grammar `[[fill]align][sign][#][0][width][,][.prec][type]`.
struct Spec {
    fill: char,
    align: Option<char>,
    sign: Option<char>,
    alternate: bool,
    zero_pad: bool,
    width: Option<usize>,
    comma: bool,
    precision: Option<usize>,
    ty: char,
}

fn parse_spec(spec: &str) -> Spec {
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut s = Spec {
        fill: ' ',
        align: None,
        sign: None,
        alternate: false,
        zero_pad: false,
        width: None,
        comma: false,
        precision: None,
        ty: '\0',
    };

    let align_chars = ['<', '>', '^', '='];

    if chars.len() >= 2 && align_chars.contains(&chars[1]) {
        s.fill = chars[0];
        s.align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && align_chars.contains(&chars[0]) {
        s.align = Some(chars[0]);
        i = 1;
    }

    if i < chars.len() && ['+', '-', ' '].contains(&chars[i]) {
        s.sign = Some(chars[i]);
        i += 1;
    }

    if i < chars.len() && chars[i] == '#' {
        s.alternate = true;
        i += 1;
    }

    if i < chars.len() && chars[i] == '0' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
        s.zero_pad = true;
        i += 1;
    }

    let width_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > width_start {
        s.width = chars[width_start..i].iter().collect::<String>().parse().ok();
    }

    if i < chars.len() && chars[i] == ',' {
        s.comma = true;
        i += 1;
    }

    if i < chars.len() && chars[i] == '.' {
        i += 1;
        let p_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        s.precision = chars[p_start..i].iter().collect::<String>().parse().ok();
    }

    if i < chars.len() {
        s.ty = chars[i];
    }

    s
}

/// `format(value, spec)` after conversion.
fn format_value(value: &Value, conversion: Option<char>, spec: &str) -> Result<String, FormatError> {
    let converted: Value = match conversion {
        Some('r') => Value::Text(value.repr()),
        Some('s') => Value::Text(value.unformatted()),
        _ => value.clone(),
    };

    if spec.is_empty() {
        return Ok(converted.unformatted());
    }

    // datetime: whole spec is a strftime format.
    if let Value::DateTime(dt) = &converted {
        return Ok(dt.format(spec).to_string());
    }

    let s = parse_spec(spec);
    let body = render_body(&converted, &s)?;
    Ok(apply_width(body, &s, &converted))
}

/// Formats the value ignoring width/alignment.
fn render_body(value: &Value, s: &Spec) -> Result<String, FormatError> {
    let unknown = |ty: char, type_name: &str| {
        Err(FormatError::UnknownVariable(format!(
            "Unknown format code '{ty}' for object of type '{type_name}'"
        )))
    };

    match value {
        Value::Text(t) => match s.ty {
            '\0' => {
                if let Some(p) = s.precision {
                    Ok(t.chars().take(p).collect())
                } else {
                    Ok(t.clone())
                }
            }
            's' => Ok(t.clone()),
            other => unknown(other, "str"),
        },
        Value::Bool(b) => render_int(&(*b as i64), s),
        Value::Int(i) => render_int(i, s),
        Value::Float(f) => render_float(*f, s),
        Value::None_ => match s.ty {
            '\0' | 's' => Ok("None".to_string()),
            other => unknown(other, "NoneType"),
        },
        Value::DateTime(_) => Ok(String::new()), // handled by strftime path
    }
}

fn sign_prefix(negative: bool, s: &Spec) -> String {
    if negative {
        "-".to_string()
    } else {
        match s.sign {
            Some('+') => "+".to_string(),
            Some(' ') => " ".to_string(),
            _ => String::new(),
        }
    }
}

fn group_digits(digits: &str, comma: bool) -> String {
    if !comma {
        return digits.to_string();
    }
    let mut out = String::new();
    let len = digits.len();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn render_int(i: &i64, s: &Spec) -> Result<String, FormatError> {
    let negative = *i < 0;
    let abs = i.unsigned_abs();

    let digits = match s.ty {
        '\0' | 'd' => abs.to_string(),
        'b' => format!("{abs:b}"),
        'o' => format!("{abs:o}"),
        'x' => format!("{abs:x}"),
        'X' => format!("{abs:X}"),
        'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' => return render_float(*i as f64, s),
        other => {
            return Err(FormatError::UnknownVariable(format!(
                "Unknown format code '{other}' for object of type 'int'"
            )))
        }
    };

    let prefix = if s.alternate {
        match s.ty {
            'b' => "0b",
            'o' => "0o",
            'x' => "0x",
            'X' => "0X",
            _ => "",
        }
    } else {
        ""
    };

    Ok(format!("{}{}{}", sign_prefix(negative, s), prefix, group_digits(&digits, s.comma)))
}

fn render_float(f: f64, s: &Spec) -> Result<String, FormatError> {
    let negative = f.is_sign_negative() && !f.is_nan();
    let abs = f.abs();

    let body = match s.ty {
        '\0' => match s.precision {
            Some(p) => {
                // Python `.4` on a float behaves like `.4g`: 4 significant
                // digits, then trailing zeros stripped.
                let g = format_g(abs, p, false);
                if g.contains('.') || g.contains('e') {
                    g
                } else {
                    format!("{:.*}", p.min(17), abs)
                }
            }
            None => python_float_repr(abs),
        },
        'f' | 'F' => format!("{:.*}", s.precision.unwrap_or(6), abs),
        'e' => format_e(abs, s.precision.unwrap_or(6), false),
        'E' => format_e(abs, s.precision.unwrap_or(6), true),
        'g' => format_g(abs, s.precision.unwrap_or(6), false),
        'G' => format_g(abs, s.precision.unwrap_or(6), true),
        '%' => {
            let p = s.precision.unwrap_or(6);
            format!("{:.*}", p, abs * 100.0)
        }
        'd' => {
            if abs.is_finite() && abs.fract() == 0.0 {
                format!("{}", abs as i64)
            } else {
                return Err(FormatError::UnknownVariable(
                    "Unknown format code 'd' for object of type 'float'".to_string(),
                ));
            }
        }
        other => {
            return Err(FormatError::UnknownVariable(format!(
                "Unknown format code '{other}' for object of type 'float'"
            )))
        }
    };

    let suffix = if s.ty == '%' { "%" } else { "" };
    let signed = format!("{}{body}{suffix}", sign_prefix(negative, s));

    if s.comma {
        let (int_part, rest) = split_int_part(&signed);
        return Ok(format!("{}{rest}", group_digits(&int_part, true)));
    }

    Ok(signed)
}

/// Splits "123.45" into ("123", ".45"); sign/prefix stays in the rest.
fn split_int_part(body: &str) -> (String, String) {
    let mut int_part = String::new();
    let mut rest = String::new();
    let mut in_rest = false;
    for c in body.chars() {
        if !in_rest && (c == '.' || !c.is_ascii_digit()) {
            in_rest = true;
        }
        if in_rest {
            rest.push(c);
        } else {
            int_part.push(c);
        }
    }
    (int_part, rest)
}

/// Python `{:e}` / `{:E}` with given precision: exponent always signed and
/// at least two digits (`4.000000e+00`).
fn format_e(v: f64, precision: usize, upper: bool) -> String {
    let s = format!("{:.*e}", precision, v);
    let (mantissa, exp) = s.split_once('e').unwrap();
    let exp_num: i32 = exp.parse().unwrap_or(0);
    let sign = if exp_num < 0 { '-' } else { '+' };
    let out = format!("{mantissa}e{sign}{:02}", exp_num.abs());
    if upper { out.to_uppercase() } else { out }
}

/// Python `{:g}` general format.
fn format_g(v: f64, precision: usize, upper: bool) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let p = if precision == 0 { 1 } else { precision };
    let exp = v.abs().log10().floor() as i32;
    if exp < -4 || exp >= p as i32 {
        strip_g_zeros(&format_e(v, p - 1, upper), true)
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip_g_zeros(&format!("{:.*}", decimals, v), false)
    }
}

fn strip_g_zeros(s: &str, scientific: bool) -> String {
    if scientific {
        if let Some((mantissa, exp)) = s.split_once('e') {
            let m = mantissa.trim_end_matches('0').trim_end_matches('.');
            return format!("{m}e{exp}");
        }
        s.to_string()
    } else if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

fn apply_width(body: String, s: &Spec, value: &Value) -> String {
    let Some(width) = s.width else { return body };

    let char_count = body.chars().count();
    if char_count >= width {
        return body;
    }

    let align = s.align.unwrap_or(if value.is_numeric() { '>' } else { '<' });
    let fill = if s.zero_pad && s.fill == ' ' { '0' } else { s.fill };

    match align {
        // zero-pad + right align: zeros go after the sign ("−0042", not
        // "00−42") — Python treats `0` as `fill='0', align='='` for numbers.
        '<' => format!("{body}{}", fill.to_string().repeat(width - char_count)),
        '>' if s.zero_pad && body.starts_with(['-', '+', ' ']) => {
            let mut chars = body.chars();
            let sign = chars.next().unwrap();
            let rest: String = chars.collect();
            format!("{sign}{}{rest}", fill.to_string().repeat(width - char_count))
        }
        '>' => format!("{}{body}", fill.to_string().repeat(width - char_count)),
        '^' => {
            let total = width - char_count;
            let left = total / 2;
            let right = total - left;
            format!(
                "{}{body}{}",
                fill.to_string().repeat(left),
                fill.to_string().repeat(right)
            )
        }
        '=' => {
            // Pad between sign/prefix and digits.
            let pad = fill.to_string().repeat(width - char_count);
            let mut idx = 0;
            for c in body.chars() {
                if c.is_ascii_digit() {
                    break;
                }
                idx += c.len_utf8();
            }
            let (head, tail) = body.split_at(idx);
            format!("{head}{pad}{tail}")
        }
        _ => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn parse_basics() {
        let segs = parse("{a}x{{y}}{b:>6}").unwrap();
        match &segs[0] {
            Segment::Field { literal, name, .. } => {
                assert!(literal.is_empty());
                assert_eq!(*name, "a");
            }
            _ => panic!("expected field"),
        }
        // `{{y}}` decodes to a literal attached to the FOLLOWING field
        match &segs[1] {
            Segment::Field { literal, name, spec, .. } => {
                assert_eq!(literal, "x{y}");
                assert_eq!(*name, "b");
                assert_eq!(*spec, ">6");
            }
            _ => panic!("expected field"),
        }
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn widths_alignments() {
        assert_eq!(vformat("{a:05d}", &d(&[("a", Value::Int(42))])).unwrap(), "00042");
        assert_eq!(vformat("{a:05d}", &d(&[("a", Value::Int(-42))])).unwrap(), "-0042");
        assert_eq!(vformat("{a:08.2f}", &d(&[("a", Value::Float(3.15))])).unwrap(), "00003.15");
        assert_eq!(vformat("{a:04X}", &d(&[("a", Value::Int(255))])).unwrap(), "00FF");
        assert_eq!(vformat("{a:^5}", &d(&[("a", Value::Text("ab".into()))])).unwrap(), " ab  ");
        assert_eq!(vformat("{a:*<5}", &d(&[("a", Value::Text("ab".into()))])).unwrap(), "ab***");
        assert_eq!(vformat("{a:0>3}", &d(&[("a", Value::Text("x".into()))])).unwrap(), "00x");
    }

    #[test]
    fn numbers() {
        assert_eq!(vformat("{a:,}", &d(&[("a", Value::Int(1000000))])).unwrap(), "1,000,000");
        assert_eq!(vformat("{a:%}", &d(&[("a", Value::Float(0.5))])).unwrap(), "50.000000%");
        assert_eq!(vformat("{a:.2%}", &d(&[("a", Value::Float(0.5))])).unwrap(), "50.00%");
        assert_eq!(vformat("{a:e}", &d(&[("a", Value::Int(4))])).unwrap(), "4.000000e+00");
        assert_eq!(vformat("{a:g}", &d(&[("a", Value::Float(100000.0))])).unwrap(), "100000");
        assert_eq!(vformat("{a:.3g}", &d(&[("a", Value::Float(1234.5678))])).unwrap(), "1.23e+03");
        assert_eq!(vformat("{a:10.4}", &d(&[("a", Value::Float(3.15))])).unwrap(), "      3.15");
        assert_eq!(vformat("{a:.0f}", &d(&[("a", Value::Float(2.5))])).unwrap(), "2");
        assert_eq!(vformat("{a:05.1f}", &d(&[("a", Value::Float(42.25))])).unwrap(), "042.2");
        assert_eq!(vformat("{a:=+8d}", &d(&[("a", Value::Int(42))])).unwrap(), "+     42");
        assert_eq!(vformat("{a:+d}", &d(&[("a", Value::Int(42))])).unwrap(), "+42");
        assert_eq!(vformat("{a: d}", &d(&[("a", Value::Int(42))])).unwrap(), " 42");
        assert_eq!(vformat("{a:#o}", &d(&[("a", Value::Int(8))])).unwrap(), "0o10");
        assert_eq!(vformat("{a:#b}", &d(&[("a", Value::Int(5))])).unwrap(), "0b101");
        assert_eq!(vformat("{a:#x}", &d(&[("a", Value::Int(255))])).unwrap(), "0xff");
        assert_eq!(vformat("{a:d}", &d(&[("a", Value::Bool(true))])).unwrap(), "1");
        assert_eq!(vformat("{a:02d}", &d(&[("a", Value::Bool(true))])).unwrap(), "01");
        assert_eq!(vformat("{a:0d}", &d(&[("a", Value::Int(1))])).unwrap(), "1");
        assert_eq!(vformat("{a:0<5}", &d(&[("a", Value::Int(42))])).unwrap(), "42000");
        assert_eq!(vformat("{a:05}", &d(&[("a", Value::Int(42))])).unwrap(), "00042");
        assert_eq!(vformat("{a:<6}", &d(&[("a", Value::Text("中文".into()))])).unwrap(), "中文    ");
    }

    #[test]
    fn conversions_and_defaults() {
        assert_eq!(vformat("{a!r}", &d(&[("a", Value::Text("x".into()))])).unwrap(), "'x'");
        assert_eq!(
            vformat("{t!r:>{w}}", &d(&[("t", Value::Text("x".into())), ("w", Value::Int(6))])).unwrap(),
            "   'x'"
        );
        assert_eq!(vformat("{f}", &d(&[("f", Value::Float(2.0))])).unwrap(), "2.0");
        assert_eq!(vformat("{b}", &d(&[("b", Value::Bool(true))])).unwrap(), "True");
        assert_eq!(vformat("{n}", &d(&[("n", Value::None_)])).unwrap(), "None");
    }

    #[test]
    fn datetime_specs() {
        let dt = NaiveDateTime::parse_from_str("2026-03-07 08:05:09", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(vformat("{d}", &d(&[("d", Value::DateTime(dt))])).unwrap(), "2026-03-07 08:05:09");
        assert_eq!(vformat("{d:%Y-%m-%d}", &d(&[("d", Value::DateTime(dt))])).unwrap(), "2026-03-07");
    }

    #[test]
    fn syntax_errors() {
        assert!(matches!(vformat("{a!}", &d(&[("a", Value::Text("x".into()))])), Err(FormatError::Syntax(_))));
        assert!(matches!(vformat("a{b}}c", &d(&[])), Err(FormatError::Syntax(_))));
        assert!(matches!(vformat("}", &d(&[])), Err(FormatError::Syntax(_))));
        assert!(matches!(vformat("{a", &d(&[("a", Value::Int(1))])), Err(FormatError::Syntax(_))));
        assert!(matches!(vformat("{", &d(&[])), Err(FormatError::Syntax(_))));
    }
}
