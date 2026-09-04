use crate::parser::{Diagnostic, Severity};

const MAX_LINE_LENGTH: usize = 120;

/// Text-level checks that do not require a successful parse, so they still
/// run (and still help) on files that fail to parse outright.
pub fn scan(input: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (i, raw) in input.lines().enumerate() {
        let number = i + 1;

        if raw.contains('\t') {
            diags.push(Diagnostic {
                line: number,
                severity: Severity::Error,
                message: "tab character found; YAML indentation must use spaces".to_string(),
            });
        }

        if raw.ends_with(' ') || raw.ends_with('\t') {
            diags.push(Diagnostic {
                line: number,
                severity: Severity::Warning,
                message: "trailing whitespace".to_string(),
            });
        }

        let len = raw.chars().count();
        if len > MAX_LINE_LENGTH {
            diags.push(Diagnostic {
                line: number,
                severity: Severity::Warning,
                message: format!("line exceeds {} characters ({})", MAX_LINE_LENGTH, len),
            });
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_tabs_and_trailing_whitespace() {
        let diags = scan("a:\tb\nc: d \n");
        assert!(diags.iter().any(|d| d.message.contains("tab")));
        assert!(diags.iter().any(|d| d.message.contains("trailing whitespace")));
    }

    #[test]
    fn clean_input_has_no_diagnostics() {
        assert!(scan("a: 1\nb: 2\n").is_empty());
    }
}
