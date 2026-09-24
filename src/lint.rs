use crate::parser::{Diagnostic, Severity};

const DEFAULT_MAX_LINE_LENGTH: usize = 120;

/// Controls which text-level lint checks run and how strict they are. A
/// caller with its own house style (longer lines, tabs on purpose, or a CI
/// job that only wants to fail on errors) builds one of these instead of
/// forking the rule set.
#[derive(Debug, Clone)]
pub struct LintConfig {
    pub check_tabs: bool,
    pub check_trailing_whitespace: bool,
    pub check_line_length: bool,
    pub max_line_length: usize,
    /// Diagnostics below this severity are dropped before `scan` returns.
    pub min_severity: Severity,
}

impl Default for LintConfig {
    fn default() -> Self {
        LintConfig {
            check_tabs: true,
            check_trailing_whitespace: true,
            check_line_length: true,
            max_line_length: DEFAULT_MAX_LINE_LENGTH,
            min_severity: Severity::Warning,
        }
    }
}

/// Text-level checks that do not require a successful parse, so they still
/// run (and still help) on files that fail to parse outright.
pub fn scan(input: &str, config: &LintConfig) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (i, raw) in input.lines().enumerate() {
        let number = i + 1;

        if config.check_tabs && raw.contains('\t') {
            diags.push(Diagnostic {
                line: number,
                severity: Severity::Error,
                message: "tab character found; YAML indentation must use spaces".to_string(),
            });
        }

        if config.check_trailing_whitespace && (raw.ends_with(' ') || raw.ends_with('\t')) {
            diags.push(Diagnostic {
                line: number,
                severity: Severity::Warning,
                message: "trailing whitespace".to_string(),
            });
        }

        if config.check_line_length {
            let len = raw.chars().count();
            if len > config.max_line_length {
                diags.push(Diagnostic {
                    line: number,
                    severity: Severity::Warning,
                    message: format!("line exceeds {} characters ({})", config.max_line_length, len),
                });
            }
        }
    }
    diags.retain(|d| d.severity >= config.min_severity);
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_tabs_and_trailing_whitespace() {
        let diags = scan("a:\tb\nc: d \n", &LintConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("tab")));
        assert!(diags.iter().any(|d| d.message.contains("trailing whitespace")));
    }

    #[test]
    fn clean_input_has_no_diagnostics() {
        assert!(scan("a: 1\nb: 2\n", &LintConfig::default()).is_empty());
    }

    #[test]
    fn max_line_length_is_configurable() {
        let input = "a: 12345\n";
        let mut config = LintConfig::default();
        config.max_line_length = 4;
        let diags = scan(input, &config);
        assert!(diags.iter().any(|d| d.message.contains("exceeds 4")));
    }

    #[test]
    fn disabled_check_produces_no_diagnostic() {
        let mut config = LintConfig::default();
        config.check_tabs = false;
        let diags = scan("a:\tb\n", &config);
        assert!(diags.is_empty());
    }

    #[test]
    fn min_severity_filters_out_warnings() {
        let mut config = LintConfig::default();
        config.min_severity = Severity::Error;
        let diags = scan("a:\tb\nc: d \n", &config);
        assert!(diags.iter().all(|d| d.severity == Severity::Error));
        assert!(diags.iter().any(|d| d.message.contains("tab")));
    }
}
