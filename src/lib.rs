//! A dependency-free library for parsing and linting YAML configuration
//! files.
//!
//! [`load`] parses a practical subset of YAML (mappings, sequences,
//! scalars, comments) into a [`Value`] tree and runs a small set of lint
//! checks (duplicate keys, tabs, trailing whitespace, long lines) over the
//! source text. The result is a [`Document`] that can be rendered either
//! for a human or as JSON via [`report::render`].

pub mod lint;
pub mod parser;
pub mod report;

pub use parser::{Diagnostic, Document, ParseError, Severity, Value};
pub use report::OutputFormat;

/// Parses `input` and merges the parser's own diagnostics (duplicate keys)
/// with the text-level lint diagnostics from [`lint::scan`], sorted by
/// line number.
pub fn load(input: &str) -> Result<Document, ParseError> {
    let mut doc = parser::parse(input)?;
    let mut extra = lint::scan(input);
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
}
