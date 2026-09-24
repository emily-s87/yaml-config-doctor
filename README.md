# yaml-config-doctor

A small Rust library for parsing YAML configuration files and pointing out
what's wrong with them, without pulling in a YAML crate or a JSON crate.
Standard library only, zero third-party dependencies.

## Why

Most YAML problems in config files are boring: a duplicate key that
silently overwrites an earlier one, a tab that snuck into indentation, a
line some editor wrapped and re-saved with trailing spaces. A full YAML
implementation (anchors, tags, multi-document streams, block folding...)
is overkill for catching those, and pulling in `serde_yaml` plus a JSON
crate is a lot of dependency surface for a config linter. This crate
parses the subset of YAML that config files actually use, and reports
diagnostics in a form a person can read or a machine can consume, your
choice.

## What it parses

- block mappings (`key: value`) and block sequences (`- item`)
- nesting through indentation, including sequences of mappings
- flow-style collections (`{a: 1, b: 2}`, `[1, 2, 3]`), including nested
  ones (`[{a: 1}, {a: 2}]`), as the value on a single line
- scalars: strings (plain, single- and double-quoted), integers, floats,
  booleans, and null (`null`, `~`, empty)
- `#` comments, including telling a real comment from a `#` inside a
  quoted string

Not supported: anchors and aliases, tags, multi-line block scalars (`|`,
`>`), multi-line flow collections, and multi-document streams. The top
level of a document must be a mapping or a sequence. Trying to parse
anything outside this subset returns a `ParseError` with a line number
rather than guessing.

## Usage

```rust
use yaml_config_doctor::{load, OutputFormat, Value, report};

fn main() {
    let input = "\
name: payments-api
port: 8080
retries: 3
servers:
  - host: a.internal
    weight: 1
  - host: b.internal
    weight: 2
";

    let doc = load(input).expect("valid config");

    // Walk the parsed tree directly.
    let name = doc.value.get("name").and_then(Value::as_str).unwrap();
    println!("service: {name}");

    // Or render a report. A caller's own CLI would typically map this
    // straight onto a --json flag: OutputFormat::Json when it's set,
    // OutputFormat::Human otherwise.
    let human = report::render(&doc, OutputFormat::Human);
    let json = report::render(&doc, OutputFormat::Json);
    print!("{human}");
    println!("{json}");
}
```

Human output lists one diagnostic per line, or `no issues found`:

```
line 4: warning: duplicate key 'port'
```

JSON output is a single object with the diagnostics and the parsed value,
so a wrapper script can pipe it into `jq` or another tool:

```json
{"diagnostics":[{"line":4,"severity":"warning","message":"duplicate key 'port'"}],"value":{"name":"payments-api","port":8080}}
```

## Lint checks

Run automatically as part of `load`:

- duplicate keys within the same mapping
- tab characters (YAML indentation must be spaces)
- trailing whitespace
- lines over 120 characters

Each diagnostic carries a line number, a severity (`warning` or `error`),
and a message, and is available regardless of which output format you
render.

Any of that is configurable through `LintConfig`, for callers with their
own house style:

```rust
use yaml_config_doctor::{load_with_lint_config, LintConfig, Severity};

let mut config = LintConfig::default();
config.max_line_length = 200;
config.check_tabs = false;
config.min_severity = Severity::Error; // drop warning-level diagnostics

let doc = load_with_lint_config("name: payments-api\n", &config).unwrap();
```

## Schema validation

`load` only checks that a file is well-formed YAML and free of the lint
issues above; it says nothing about whether the *shape* is right. `schema`
covers that:

```rust
use yaml_config_doctor::{load, validate, Field, Schema};

let doc = load("name: payments-api\nport: 8080\n").unwrap();

let schema = Schema::Mapping(vec![
    Field::required("name", Schema::String),
    Field::required("port", Schema::Int),
    Field::optional("retries", Schema::Int),
]);

let errors = validate(&doc.value, &schema);
assert!(errors.is_empty());
```

A mismatch comes back as a `ValidationError` with a `$`-rooted path
(`$.servers[1].port`) and a message, and `validate` collects every
violation in the tree rather than stopping at the first one. Available
schema shapes: `Any`, `Null`, `Bool`, `Int`, `Float` (also accepts an
`Int`), `String`, `Sequence(Box<Schema>)`, `Mapping(Vec<Field>)`, and
`OneOf(Vec<Schema>)` for a value that just needs to match one of several
shapes.

## Status

Early. The parser covers the common shapes of hand-written config files;
it is not a YAML-spec-compliant implementation and doesn't try to be.
