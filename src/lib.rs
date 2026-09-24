//! A dependency-free library for parsing and linting YAML configuration
//! files.
//!
//! [`load`] parses a practical subset of YAML (mappings, sequences, flow
//! collections, scalars, comments) into a [`Value`] tree and runs a small
//! set of lint checks (duplicate keys, tabs, trailing whitespace, long
//! lines) over the source text. The result is a [`Document`] that can be
//! rendered either for a human or as JSON via [`report::render`].
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
}
