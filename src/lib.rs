//! A dependency-free library for parsing and linting YAML configuration
//! files.
//!
//! [`load`] parses a practical subset of YAML (mappings, sequences, flow
//! collections, scalars, comments) into a [`Value`] tree and runs a small
//! set of lint checks (duplicate keys, tabs, trailing whitespace, long
//! lines) over the source text. The result is a [`Document`] that can be
//! rendered either for a human or as JSON via [`report::render`].
//! [`load_all`] does the same for a `---`-separated stream of documents.
//!
//! [`schema::validate`] separately checks a parsed [`Value`] against a
//! [`Schema`] describing the shape a config is expected to have (required
//! fields, field types), for callers that want more than "it parsed".

pub mod lint;
pub mod parser;
pub mod report;
pub mod schema;

pub use lint::LintConfig;
pub use parser::{Diagnostic, Document, ParseError, Severity, Value};
pub use report::OutputFormat;
pub use schema::{validate, Field, Schema, ValidationError};

/// Parses `input` and merges the parser's own diagnostics (duplicate keys)
/// with the text-level lint diagnostics from [`lint::scan`] (using default
/// lint settings), sorted by line number. Use [`load_with_lint_config`] to
/// tune or disable individual lint rules.
pub fn load(input: &str) -> Result<Document, ParseError> {
    load_with_lint_config(input, &LintConfig::default())
}

/// Like [`load`], but runs the text-level lint checks with a caller-supplied
/// [`LintConfig`] instead of the defaults.
pub fn load_with_lint_config(input: &str, lint_config: &LintConfig) -> Result<Document, ParseError> {
    let mut doc = parser::parse(input)?;
    let mut extra = lint::scan(input, lint_config);
    doc.diagnostics.append(&mut extra);
    doc.diagnostics.sort_by_key(|d| d.line);
    Ok(doc)
}

/// Parses `input` as a `---`-separated stream of one or more documents and
/// runs the same lint pass as [`load`] over the whole file, routing each
/// diagnostic to whichever document its line falls in. Use this instead of
/// [`load`] for input that may contain more than one document.
pub fn load_all(input: &str) -> Result<Vec<Document>, ParseError> {
    load_all_with_lint_config(input, &LintConfig::default())
}

/// Like [`load_all`], but runs the text-level lint checks with a
/// caller-supplied [`LintConfig`] instead of the defaults.
pub fn load_all_with_lint_config(input: &str, lint_config: &LintConfig) -> Result<Vec<Document>, ParseError> {
    let mut docs = parser::parse_all(input)?;
    let ranges = parser::document_ranges(input);
    for diag in lint::scan(input, lint_config) {
        if let Some(idx) = ranges.iter().position(|&(start, end)| start <= diag.line && diag.line <= end) {
            docs[idx].diagnostics.push(diag);
        }
    }
    for doc in &mut docs {
        doc.diagnostics.sort_by_key(|d| d.line);
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_combines_parser_and_lint_diagnostics() {
        let input = "a: 1\na: 2\t\n";
        let doc = load(input).unwrap();
        assert!(doc.diagnostics.iter().any(|d| d.message.contains("duplicate")));
        assert!(doc.diagnostics.iter().any(|d| d.message.contains("tab")));
    }

    #[test]
    fn render_human_and_json_agree_on_content() {
        let doc = load("name: demo\n").unwrap();
        let human = report::render(&doc, OutputFormat::Human);
        let json = report::render(&doc, OutputFormat::Json);
        assert_eq!(human, "no issues found\n");
        assert!(json.contains("\"name\""));
    }

    #[test]
    fn load_with_lint_config_can_suppress_lint_warnings() {
        let input = "a: 1\nb: 2 \n";
        let mut config = LintConfig::default();
        config.check_trailing_whitespace = false;
        let doc = load_with_lint_config(input, &config).unwrap();
        assert!(!doc.diagnostics.iter().any(|d| d.message.contains("trailing whitespace")));
    }

    #[test]
    fn load_with_lint_config_can_raise_min_severity() {
        let input = "a: 1\nb: 2 \n";
        let mut config = LintConfig::default();
        config.min_severity = Severity::Error;
        let doc = load_with_lint_config(input, &config).unwrap();
        assert!(doc.diagnostics.is_empty());
    }

    #[test]
    fn load_treats_a_lone_leading_marker_as_a_single_document() {
        let doc = load("---\nname: demo\n").unwrap();
        assert_eq!(doc.value.get("name").and_then(Value::as_str), Some("demo"));
    }

    #[test]
    fn load_all_parses_a_multi_document_stream() {
        let docs = load_all("a: 1\n---\nb: 2\n").unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].value.get("a"), Some(&Value::Int(1)));
        assert_eq!(docs[1].value.get("b"), Some(&Value::Int(2)));
    }

    #[test]
    fn load_all_routes_lint_diagnostics_to_the_right_document() {
        let input = "a: 1\t\n---\nb: 2 \n";
        let docs = load_all(input).unwrap();
        assert!(docs[0].diagnostics.iter().any(|d| d.message.contains("tab")));
        assert!(docs[1].diagnostics.iter().any(|d| d.message.contains("trailing whitespace")));
    }
}
