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

/// Ordered `Warning < Error` so a minimum-severity filter can compare
/// against it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

/// Flow collections (`{...}`, `[...]`) are recognized by their leading
/// bracket before anything else, since braces make `split_colon`'s "first
/// unquoted colon" rule unreliable - `{a: 1}` would otherwise look like a
/// nested block mapping.
fn is_flow_start(s: &str) -> bool {
    matches!(s.chars().next(), Some('{') | Some('['))
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
            parse_scalar(value_raw, number, diags).map_err(|message| ParseError { line: number, message })?
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
        } else if is_flow_start(rest) {
            if !nested.is_empty() {
                return Err(ParseError {
                    line: nested[0].number,
                    message: "unexpected indentation after flow collection".to_string(),
                });
            }
            parse_scalar(rest, number, diags).map_err(|message| ParseError { line: number, message })?
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
            parse_scalar(rest, number, diags).map_err(|message| ParseError { line: number, message })?
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

/// Parses a scalar or, if `raw` opens with `{` or `[`, a whole flow
/// collection. Flow collections are single-line only in this crate: the
/// bracketed text handed in here is everything left on the line once the
/// enclosing key or dash has been stripped off.
fn parse_scalar(raw: &str, line: usize, diags: &mut Vec<Diagnostic>) -> Result<Value, String> {
    let s = raw.trim();
    if is_flow_start(s) {
        return FlowParser::new(s, line, diags).parse_top();
    }
    if let Some(u) = unquote(s) {
        return Ok(Value::String(u));
    }
    Ok(interpret_plain_scalar(s))
}

/// Scalar-type inference shared by block scalars and the bare tokens found
/// inside flow collections. Assumes quoting has already been handled by
/// the caller.
fn interpret_plain_scalar(s: &str) -> Value {
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

/// A small recursive-descent parser for flow-style collections
/// (`{a: 1, b: 2}`, `[1, 2, 3]`), including ones nested inside each other.
/// It only ever runs over a single already-extracted line of text, so
/// there is no indentation tracking here - just brackets, commas, colons,
/// and quotes.
struct FlowParser<'a, 'd> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
    diags: &'d mut Vec<Diagnostic>,
}

impl<'a, 'd> FlowParser<'a, 'd> {
    fn new(s: &'a str, line: usize, diags: &'d mut Vec<Diagnostic>) -> Self {
        FlowParser { chars: s.chars().peekable(), line, diags }
    }

    fn parse_top(&mut self) -> Result<Value, String> {
        let value = self.parse_value()?;
        self.skip_ws();
        if self.chars.peek().is_some() {
            return Err("unexpected trailing content after flow collection".to_string());
        }
        Ok(value)
    }

    fn skip_ws(&mut self) {
        while matches!(self.chars.peek(), Some(c) if c.is_whitespace()) {
            self.chars.next();
        }
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.chars.peek() {
            Some('{') => self.parse_mapping(),
            Some('[') => self.parse_sequence(),
            Some('"') | Some('\'') => Ok(Value::String(self.parse_quoted()?)),
            Some(_) => Ok(interpret_plain_scalar(&self.parse_token())),
            None => Err("unexpected end of input in flow value".to_string()),
        }
    }

    fn parse_mapping(&mut self) -> Result<Value, String> {
        self.chars.next(); // '{'
        let mut entries: Vec<(String, Value)> = Vec::new();
        self.skip_ws();
        if self.chars.peek() == Some(&'}') {
            self.chars.next();
            return Ok(Value::Mapping(entries));
        }
        loop {
            self.skip_ws();
            let key = match self.chars.peek() {
                Some('"') | Some('\'') => self.parse_quoted()?,
                _ => self.parse_token(),
            };
            if key.is_empty() {
                return Err("expected a key in flow mapping".to_string());
            }
            self.skip_ws();
            match self.chars.next() {
                Some(':') => {}
                other => return Err(format!("expected ':' in flow mapping, found {:?}", other)),
            }
            let value = self.parse_value()?;
            if entries.iter().any(|(k, _)| k == &key) {
                self.diags.push(Diagnostic {
                    line: self.line,
                    severity: Severity::Warning,
                    message: format!("duplicate key '{}'", key),
                });
            }
            entries.push((key, value));
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some('}') => break,
                other => return Err(format!("expected ',' or '}}' in flow mapping, found {:?}", other)),
            }
        }
        Ok(Value::Mapping(entries))
    }

    fn parse_sequence(&mut self) -> Result<Value, String> {
        self.chars.next(); // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.chars.peek() == Some(&']') {
            self.chars.next();
            return Ok(Value::Sequence(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some(']') => break,
                other => return Err(format!("expected ',' or ']' in flow sequence, found {:?}", other)),
            }
        }
        Ok(Value::Sequence(items))
    }

    fn parse_quoted(&mut self) -> Result<String, String> {
        let quote = self.chars.next().unwrap();
        let mut out = String::new();
        loop {
            match self.chars.next() {
                None => return Err("unterminated quoted string in flow collection".to_string()),
                Some(c) if c == quote => {
                    if quote == '\'' && self.chars.peek() == Some(&'\'') {
                        self.chars.next();
                        out.push('\'');
                        continue;
                    }
                    break;
                }
                Some('\\') if quote == '"' => match self.chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => return Err("unterminated quoted string in flow collection".to_string()),
                },
                Some(c) => out.push(c),
            }
        }
        Ok(out)
    }

    /// Reads a bare (unquoted) token up to the next flow delimiter. A
    /// colon only ends the token when it is acting as a key/value
    /// separator (followed by whitespace, a delimiter, or the end of
    /// input) so that plain scalars like `http://host` survive intact.
    fn parse_token(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.chars.peek() {
                None => break,
                Some(',') | Some('}') | Some(']') => break,
                Some(':') => {
                    let mut lookahead = self.chars.clone();
                    lookahead.next();
                    match lookahead.peek() {
                        None => break,
                        Some(c) if c.is_whitespace() || matches!(c, ',' | '}' | ']') => break,
                        _ => {
                            out.push(':');
                            self.chars.next();
                        }
                    }
                }
                Some(&c) => {
                    out.push(c);
                    self.chars.next();
                }
            }
        }
        out.trim().to_string()
    }
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

    #[test]
    fn parses_flow_mapping() {
        let doc = parse("a: {b: 1, c: two, d: true}\n").unwrap();
        let a = doc.value.get("a").unwrap();
        assert_eq!(a.get("b"), Some(&Value::Int(1)));
        assert_eq!(a.get("c").and_then(Value::as_str), Some("two"));
        assert_eq!(a.get("d"), Some(&Value::Bool(true)));
    }

    #[test]
    fn parses_flow_sequence() {
        let doc = parse("a: [1, 2, 3]\n").unwrap();
        let a = doc.value.get("a").and_then(Value::as_sequence).unwrap();
        assert_eq!(a, &[Value::Int(1), Value::Int(2), Value::Int(3)][..]);
    }

    #[test]
    fn parses_nested_flow_collections() {
        let doc = parse("servers: [{host: a, port: 1}, {host: b, port: 2}]\n").unwrap();
        let servers = doc.value.get("servers").and_then(Value::as_sequence).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[1].get("host").and_then(Value::as_str), Some("b"));
    }

    #[test]
    fn parses_flow_collection_as_block_sequence_item() {
        let doc = parse("- [1, 2]\n- {a: 1}\n").unwrap();
        let items = doc.value.as_sequence().unwrap();
        assert_eq!(items[0].as_sequence(), Some(&[Value::Int(1), Value::Int(2)][..]));
        assert_eq!(items[1].get("a"), Some(&Value::Int(1)));
    }

    #[test]
    fn flow_scalars_can_contain_unquoted_colons() {
        let doc = parse("a: [http://host, 1]\n").unwrap();
        let a = doc.value.get("a").and_then(Value::as_sequence).unwrap();
        assert_eq!(a[0], Value::String("http://host".to_string()));
    }

    #[test]
    fn flags_duplicate_keys_within_a_flow_mapping() {
        let doc = parse("a: {b: 1, b: 2}\n").unwrap();
        assert!(doc.diagnostics.iter().any(|d| d.message.contains("duplicate")));
    }

    #[test]
    fn rejects_unterminated_flow_mapping() {
        let err = parse("a: {b: 1\n").unwrap_err();
        assert_eq!(err.line, 1);
    }
}
