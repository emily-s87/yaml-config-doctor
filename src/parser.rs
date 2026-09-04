use std::fmt;

/// A parsed YAML value. Only the subset of YAML actually handled by this
/// crate is represented here: no anchors, aliases, tags, or multi-document
/// streams.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Sequence(Vec<Value>),
    Mapping(Vec<(String, Value)>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_mapping(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Mapping(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_sequence(&self) -> Option<&[Value]> {
        match self {
            Value::Sequence(s) => Some(s),
            _ => None,
        }
    }

    /// Looks up a key if this value is a mapping. Mappings preserve
    /// insertion order rather than hashing, since config files are small
    /// and order-preserving output makes diffs and JSON dumps stable.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_mapping()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// The result of a successful parse: the value tree plus any warnings
/// collected along the way (currently just duplicate-key detection; text
/// level lint rules live in the `lint` module).
pub struct Document {
    pub value: Value,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    indent: usize,
    content: &'a str,
    number: usize,
}

pub fn parse(input: &str) -> Result<Document, ParseError> {
    let mut diagnostics = Vec::new();
    let lines = preprocess(input);
    if lines.is_empty() {
        return Ok(Document { value: Value::Null, diagnostics });
    }
    let (value, consumed) = parse_block(&lines, &mut diagnostics)?;
    if consumed != lines.len() {
        return Err(ParseError {
            line: lines[consumed].number,
            message: "unexpected indentation".to_string(),
        });
    }
    Ok(Document { value, diagnostics })
}

fn preprocess(input: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for (i, raw) in input.lines().enumerate() {
        let number = i + 1;
        let no_comment = strip_comment(raw);
        let trimmed = no_comment.trim_end();
        if trimmed.trim_start().is_empty() {
            continue;
        }
        let indent = trimmed.len() - trimmed.trim_start().len();
        out.push(Line { indent, content: &trimmed[indent..], number });
    }
    out
}

/// Cuts off a trailing `#` comment, but only when the `#` sits outside a
/// quoted string and is preceded by whitespace or starts the line - the
/// same rule the YAML spec uses to tell a comment from a literal `#`.
fn strip_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => {
                let at_start = idx == 0;
                let after_space = line[..idx].ends_with(char::is_whitespace);
                if at_start || after_space {
                    return &line[..idx];
                }
            }
            _ => {}
        }
    }
    line
}

fn is_sequence_item(content: &str) -> bool {
    content == "-" || content.starts_with("- ")
}

fn parse_block(lines: &[Line], diags: &mut Vec<Diagnostic>) -> Result<(Value, usize), ParseError> {
    if lines.is_empty() {
        return Ok((Value::Null, 0));
    }
    let indent = lines[0].indent;
    if is_sequence_item(lines[0].content) {
        parse_sequence(lines, indent, diags)
    } else {
        parse_mapping(lines, indent, diags)
    }
}

fn nested_slice<'a>(lines: &[Line<'a>], start: usize, indent: usize) -> (&[Line<'a>], usize) {
    let mut j = start;
    while j < lines.len() && lines[j].indent > indent {
        j += 1;
    }
    (&lines[start..j], j)
}

fn parse_mapping(lines: &[Line], indent: usize, diags: &mut Vec<Diagnostic>) -> Result<(Value, usize), ParseError> {
    let mut entries: Vec<(String, Value)> = Vec::new();
    let mut i = 0;
    while i < lines.len() && lines[i].indent == indent {
        if is_sequence_item(lines[i].content) {
            break;
        }
        let content = lines[i].content;
        let number = lines[i].number;
        let (key_raw, value_raw) = split_colon(content).ok_or_else(|| ParseError {
            line: number,
            message: format!("expected 'key: value', found '{}'", content),
        })?;
        let key = unquote(key_raw).unwrap_or_else(|| key_raw.to_string());

        if entries.iter().any(|(k, _)| k == &key) {
            diags.push(Diagnostic {
                line: number,
                severity: Severity::Warning,
                message: format!("duplicate key '{}'", key),
            });
        }

        let (nested, next) = nested_slice(lines, i + 1, indent);

        let value = if !value_raw.is_empty() {
            if !nested.is_empty() {
                return Err(ParseError {
                    line: nested[0].number,
                    message: format!("unexpected indentation after '{}'", key),
                });
            }
            parse_scalar(value_raw)
        } else if nested.is_empty() {
            Value::Null
        } else {
            let (v, consumed) = parse_block(nested, diags)?;
            if consumed != nested.len() {
                return Err(ParseError {
                    line: nested[consumed].number,
                    message: "unexpected indentation".to_string(),
                });
            }
            v
        };

        entries.push((key, value));
        i = next;
    }
    Ok((Value::Mapping(entries), i))
}

fn parse_sequence(lines: &[Line], indent: usize, diags: &mut Vec<Diagnostic>) -> Result<(Value, usize), ParseError> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() && lines[i].indent == indent && is_sequence_item(lines[i].content) {
        let content = lines[i].content;
        let number = lines[i].number;
        let after_dash = &content[1..];
        let rest = after_dash.trim_start();
        let dash_and_spaces = after_dash.len() - rest.len() + 1;
        let synthetic_indent = indent + dash_and_spaces;

        let (nested, next) = nested_slice(lines, i + 1, indent);

        let item = if rest.is_empty() {
            if nested.is_empty() {
                Value::Null
            } else {
                let (v, consumed) = parse_block(nested, diags)?;
                if consumed != nested.len() {
                    return Err(ParseError {
                        line: nested[consumed].number,
                        message: "unexpected indentation".to_string(),
                    });
                }
                v
            }
        } else if is_sequence_item(rest) || split_colon(rest).is_some() {
            let synthetic = Line { indent: synthetic_indent, content: rest, number };
            let mut combined = Vec::with_capacity(nested.len() + 1);
            combined.push(synthetic);
            combined.extend_from_slice(nested);
            let (v, consumed) = parse_block(&combined, diags)?;
            if consumed != combined.len() {
                return Err(ParseError {
                    line: combined[consumed].number,
                    message: "unexpected indentation".to_string(),
                });
            }
            v
        } else {
            if !nested.is_empty() {
                return Err(ParseError {
                    line: nested[0].number,
                    message: "unexpected indentation after scalar sequence item".to_string(),
                });
            }
            parse_scalar(rest)
        };

        items.push(item);
        i = next;
    }
    Ok((Value::Sequence(items), i))
}

/// Finds the first unquoted `:` that separates a mapping key from its
/// value (a colon only counts if it is followed by a space or the end of
/// the string, matching YAML's block-mapping rule).
fn split_colon(s: &str) -> Option<(&str, &str)> {
    let mut in_single = false;
    let mut in_double = false;
    for (idx, ch) in s.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ':' if !in_single && !in_double => {
                let after = &s[idx + 1..];
                if after.is_empty() || after.starts_with(' ') {
                    return Some((s[..idx].trim(), after.trim_start()));
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_scalar(raw: &str) -> Value {
    let s = raw.trim();
    if let Some(u) = unquote(s) {
        return Value::String(u);
    }
    match s {
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        "null" | "Null" | "NULL" | "~" | "" => return Value::Null,
        _ => {}
    }
    if let Ok(i) = s.parse::<i64>() {
        return Value::Int(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Value::Float(f);
    }
    Value::String(s.to_string())
}

fn unquote(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return Some(out);
    }
    if bytes.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        let inner = &s[1..s.len() - 1];
        return Some(inner.replace("''", "'"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_mapping() {
        let doc = parse("name: demo\nport: 8080\ndebug: true\n").unwrap();
        assert_eq!(doc.value.get("name").and_then(Value::as_str), Some("demo"));
        assert_eq!(doc.value.get("port"), Some(&Value::Int(8080)));
        assert_eq!(doc.value.get("debug"), Some(&Value::Bool(true)));
    }

    #[test]
    fn parses_nested_sequence_of_mappings() {
        let input = "servers:\n  - name: a\n    port: 1\n  - name: b\n    port: 2\n";
        let doc = parse(input).unwrap();
        let servers = doc.value.get("servers").and_then(Value::as_sequence).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[1].get("name").and_then(Value::as_str), Some("b"));
    }

    #[test]
    fn flags_duplicate_keys() {
        let doc = parse("a: 1\na: 2\n").unwrap();
        assert!(doc.diagnostics.iter().any(|d| d.message.contains("duplicate")));
    }

    #[test]
    fn strips_trailing_comments_but_not_quoted_hashes() {
        let doc = parse("a: \"b#c\" # a real comment\n").unwrap();
        assert_eq!(doc.value.get("a").and_then(Value::as_str), Some("b#c"));
    }

    #[test]
    fn rejects_malformed_line() {
        let err = parse("not a mapping line").unwrap_err();
        assert_eq!(err.line, 1);
    }
}
