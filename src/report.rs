use crate::parser::{Document, Severity, Value};

/// Which shape to render a `Document` as. This is the hook a caller's own
/// CLI (or web handler, or test harness) wires up to something like a
/// `--json` flag: pick `Human` for a terminal and `Json` for machine
/// consumers, same document either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

pub fn render(document: &Document, format: OutputFormat) -> String {
    match format {
        OutputFormat::Human => render_human(document),
        OutputFormat::Json => render_json(document),
    }
}

fn render_human(document: &Document) -> String {
    let mut out = String::new();
    if document.diagnostics.is_empty() {
        out.push_str("no issues found\n");
    } else {
        for d in &document.diagnostics {
            out.push_str(&format!("line {}: {}: {}\n", d.line, d.severity, d.message));
        }
    }
    out
}

fn render_json(document: &Document) -> String {
    let mut out = String::new();
    out.push_str("{\"diagnostics\":[");
    for (i, d) in document.diagnostics.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let severity = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        out.push_str(&format!(
            "{{\"line\":{},\"severity\":\"{}\",\"message\":{}}}",
            d.line,
            severity,
            json_string(&d.message)
        ));
    }
    out.push_str("],\"value\":");
    out.push_str(&json_value(&document.value));
    out.push('}');
    out
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => json_string(s),
        Value::Sequence(items) => {
            let parts: Vec<String> = items.iter().map(json_value).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Mapping(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{}:{}", json_string(k), json_value(v)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn human_output_lists_diagnostics() {
        let doc = parse("a: 1\na: 2\n").unwrap();
        let text = render(&doc, OutputFormat::Human);
        assert!(text.contains("warning"));
        assert!(text.contains("duplicate"));
    }

    #[test]
    fn json_output_is_well_formed_enough_to_scan() {
        let doc = parse("a: 1\nb: \"x\\\"y\"\n").unwrap();
        let text = render(&doc, OutputFormat::Json);
        assert!(text.starts_with("{\"diagnostics\":["));
        assert!(text.contains("\"value\":{"));
        assert!(text.contains("\\\"y"));
    }

    #[test]
    fn clean_document_has_empty_diagnostics_array() {
        let doc = parse("a: 1\n").unwrap();
        let text = render(&doc, OutputFormat::Json);
        assert!(text.contains("\"diagnostics\":[]"));
    }
}
