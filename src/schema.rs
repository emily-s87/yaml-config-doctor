use crate::parser::Value;

/// Describes the expected shape of a `Value` tree. This lets a caller check
/// structure (required fields, field types, item types) in one pass instead
/// of scattering `.get()` and `matches!` calls through their own code.
#[derive(Debug, Clone)]
pub enum Schema {
    Any,
    Null,
    Bool,
    Int,
    /// Accepts both `Value::Float` and `Value::Int`, since a plain scalar
    /// like `3` is a perfectly good value wherever a float is expected.
    Float,
    String,
    Sequence(Box<Schema>),
    Mapping(Vec<Field>),
    /// Valid if the value matches at least one of the given schemas.
    OneOf(Vec<Schema>),
}

/// One expected entry in a `Schema::Mapping`.
#[derive(Debug, Clone)]
pub struct Field {
    pub key: String,
    pub schema: Schema,
    pub required: bool,
}

impl Field {
    pub fn required(key: &str, schema: Schema) -> Field {
        Field { key: key.to_string(), schema, required: true }
    }

    pub fn optional(key: &str, schema: Schema) -> Field {
        Field { key: key.to_string(), schema, required: false }
    }
}

/// A single schema violation, located with a `$`-rooted path like
/// `$.servers[1].port` so it can be pointed at directly in an error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

/// Checks `value` against `schema` and returns every violation found. An
/// empty vector means the value matches. Unlike parsing, validation never
/// stops at the first problem, so a caller can report everything wrong with
/// a config in one shot.
pub fn validate(value: &Value, schema: &Schema) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    check(value, schema, "$", &mut errors);
    errors
}

fn check(value: &Value, schema: &Schema, path: &str, errors: &mut Vec<ValidationError>) {
    match schema {
        Schema::Any => {}
        Schema::Null => {
            if !matches!(value, Value::Null) {
                errors.push(mismatch(path, "null", value));
            }
        }
        Schema::Bool => {
            if !matches!(value, Value::Bool(_)) {
                errors.push(mismatch(path, "bool", value));
            }
        }
        Schema::Int => {
            if !matches!(value, Value::Int(_)) {
                errors.push(mismatch(path, "int", value));
            }
        }
        Schema::Float => {
            if !matches!(value, Value::Float(_) | Value::Int(_)) {
                errors.push(mismatch(path, "float", value));
            }
        }
        Schema::String => {
            if !matches!(value, Value::String(_)) {
                errors.push(mismatch(path, "string", value));
            }
        }
        Schema::Sequence(item_schema) => match value {
            Value::Sequence(items) => {
                for (i, item) in items.iter().enumerate() {
                    check(item, item_schema, &format!("{path}[{i}]"), errors);
                }
            }
            _ => errors.push(mismatch(path, "sequence", value)),
        },
        Schema::Mapping(fields) => match value {
            Value::Mapping(entries) => {
                for field in fields {
                    match entries.iter().find(|(k, _)| k == &field.key) {
                        Some((_, v)) => check(v, &field.schema, &format!("{path}.{}", field.key), errors),
                        None if field.required => errors.push(ValidationError {
                            path: path.to_string(),
                            message: format!("missing required field '{}'", field.key),
                        }),
                        None => {}
                    }
                }
            }
            _ => errors.push(mismatch(path, "mapping", value)),
        },
        Schema::OneOf(alternatives) => {
            let matches = alternatives.iter().any(|s| validate(value, s).is_empty());
            if !matches {
                errors.push(ValidationError {
                    path: path.to_string(),
                    message: format!("{} does not match any allowed schema", type_name(value)),
                });
            }
        }
    }
}

fn mismatch(path: &str, expected: &str, value: &Value) -> ValidationError {
    ValidationError {
        path: path.to_string(),
        message: format!("expected {}, found {}", expected, type_name(value)),
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::Sequence(_) => "sequence",
        Value::Mapping(_) => "mapping",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn accepts_a_matching_document() {
        let doc = parse("name: demo\nport: 8080\n").unwrap();
        let schema = Schema::Mapping(vec![
            Field::required("name", Schema::String),
            Field::required("port", Schema::Int),
        ]);
        assert!(validate(&doc.value, &schema).is_empty());
    }

    #[test]
    fn reports_missing_required_field() {
        let doc = parse("name: demo\n").unwrap();
        let schema = Schema::Mapping(vec![
            Field::required("name", Schema::String),
            Field::required("port", Schema::Int),
        ]);
        let errors = validate(&doc.value, &schema);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, "$");
        assert!(errors[0].message.contains("port"));
    }

    #[test]
    fn optional_field_is_not_required() {
        let doc = parse("name: demo\n").unwrap();
        let schema = Schema::Mapping(vec![
            Field::required("name", Schema::String),
            Field::optional("port", Schema::Int),
        ]);
        assert!(validate(&doc.value, &schema).is_empty());
    }

    #[test]
    fn reports_type_mismatch_with_path() {
        let doc = parse("port: not-a-number\n").unwrap();
        let schema = Schema::Mapping(vec![Field::required("port", Schema::Int)]);
        let errors = validate(&doc.value, &schema);
        assert_eq!(errors, vec![ValidationError {
            path: "$.port".to_string(),
            message: "expected int, found string".to_string(),
        }]);
    }

    #[test]
    fn checks_sequence_items_and_reports_indexed_path() {
        let doc = parse("servers:\n  - host: a\n    port: 1\n  - host: b\n    port: bad\n").unwrap();
        let server_schema = Schema::Mapping(vec![
            Field::required("host", Schema::String),
            Field::required("port", Schema::Int),
        ]);
        let schema = Schema::Mapping(vec![Field::required(
            "servers",
            Schema::Sequence(Box::new(server_schema)),
        )]);
        let errors = validate(&doc.value, &schema);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, "$.servers[1].port");
    }

    #[test]
    fn float_schema_also_accepts_int() {
        let doc = parse("ratio: 1\n").unwrap();
        let schema = Schema::Mapping(vec![Field::required("ratio", Schema::Float)]);
        assert!(validate(&doc.value, &schema).is_empty());
    }

    #[test]
    fn one_of_accepts_any_matching_alternative() {
        let doc = parse("timeout: 30\n").unwrap();
        let schema = Schema::Mapping(vec![Field::required(
            "timeout",
            Schema::OneOf(vec![Schema::Int, Schema::String]),
        )]);
        assert!(validate(&doc.value, &schema).is_empty());
    }

    #[test]
    fn one_of_reports_when_no_alternative_matches() {
        let doc = parse("timeout: true\n").unwrap();
        let schema = Schema::Mapping(vec![Field::required(
            "timeout",
            Schema::OneOf(vec![Schema::Int, Schema::String]),
        )]);
        let errors = validate(&doc.value, &schema);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, "$.timeout");
    }

    #[test]
    fn top_level_type_mismatch_uses_root_path() {
        let doc = parse("- 1\n- 2\n").unwrap();
        let schema = Schema::Mapping(vec![]);
        let errors = validate(&doc.value, &schema);
        assert_eq!(errors, vec![ValidationError {
            path: "$".to_string(),
            message: "expected mapping, found sequence".to_string(),
        }]);
    }
}
