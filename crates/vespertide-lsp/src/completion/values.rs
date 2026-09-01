//! Concrete completion values: schema literals and cross-file table lookups.

use std::{collections::BTreeSet, ops::Range};

use super::{CompletionItemKind, DomainCompletion};
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

const COLUMN_TYPES: &[&str] = &[
    "small_int",
    "integer",
    "big_int",
    "real",
    "double_precision",
    "text",
    "boolean",
    "date",
    "time",
    "timestamp",
    "timestamptz",
    "interval",
    "bytea",
    "uuid",
    "json",
    "inet",
    "cidr",
    "macaddr",
    "xml",
];

const REFERENCE_ACTIONS: &[&str] = &[
    "cascade",
    "restrict",
    "set_null",
    "set_default",
    "no_action",
];

/// Top-level keys of a Vespertide model file. Ordered by typical write
/// order so completion ranking matches the schema layout.
const TABLE_TOP_LEVEL_KEYS: &[(&str, &str)] = &[
    ("$schema", "JSON Schema reference for IDE validation"),
    ("name", "Table name"),
    ("columns", "Column definitions"),
    ("constraints", "Table-level constraints (CHECK, etc.)"),
    ("indexes", "Composite indexes"),
    ("comment", "Optional table comment"),
];

/// Keys inside a `columns[N]` object.
const COLUMN_OBJECT_KEYS: &[(&str, &str)] = &[
    ("name", "Column name"),
    ("type", "Column type (string or {kind: ...} object)"),
    ("nullable", "Whether NULL is allowed"),
    ("default", "Default value expression"),
    (
        "primary_key",
        "Mark as primary key (composite if used on >1 column)",
    ),
    ("unique", "Add a UNIQUE constraint"),
    ("index", "Add an index"),
    ("foreign_key", "Foreign-key reference"),
    ("comment", "Optional column comment"),
];

/// Keys inside a `foreign_key` object.
const FOREIGN_KEY_OBJECT_KEYS: &[(&str, &str)] = &[
    ("ref_table", "Referenced table name"),
    ("ref_columns", "Referenced column list"),
    ("on_delete", "ON DELETE action"),
    ("on_update", "ON UPDATE action"),
    ("name", "Optional constraint name"),
];

/// Keys inside a complex `type` object (varchar/numeric/enum/custom).
const TYPE_OBJECT_KEYS: &[(&str, &str)] = &[
    ("kind", "Type kind: varchar, char, numeric, enum, custom"),
    ("length", "Length (varchar/char)"),
    ("precision", "Precision (numeric)"),
    ("scale", "Scale (numeric)"),
    ("name", "Enum type name"),
    ("values", "Enum values"),
    ("custom_type", "Raw SQL type (kind=custom)"),
];

const CHECK_EXPR_OPERATORS_AND_KEYWORDS: &[(&str, &str)] = &[
    ("=", "Equals"),
    ("!=", "Not equals"),
    ("<>", "SQL not equals"),
    ("<", "Less than"),
    ("<=", "Less than or equal"),
    (">", "Greater than"),
    (">=", "Greater than or equal"),
    ("IN", "Value is in a list"),
    ("NOT IN", "Value is not in a list"),
    ("BETWEEN", "Value falls within a range"),
    ("IS NULL", "Value is NULL"),
    ("IS NOT NULL", "Value is not NULL"),
    ("AND", "Combine with another predicate"),
    ("OR", "Alternative predicate"),
];

const CHECK_EXPR_OPERAND_HELPERS: &[(&str, &str)] = &[
    ("NOT", "Negate the next predicate"),
    ("(", "Start a grouped predicate"),
];

/// Scalar `type` completions valid inside a JSON/YAML string literal —
/// no replacement metadata, the client just inserts at the cursor.
pub(super) fn column_types_simple() -> Vec<DomainCompletion> {
    COLUMN_TYPES
        .iter()
        .map(|column_type| value(column_type, format!("Column type: {column_type}")))
        .collect()
}

/// Full `type` completions for a string-literal slot. Simple types insert
/// in place; complex object kinds (varchar/char/numeric/enum) replace the
/// **entire** string node — quotes and current content — with a full
/// object literal so the JSON parses cleanly.
pub(super) fn column_types_in_string(
    string_byte_range: std::ops::Range<usize>,
) -> Vec<DomainCompletion> {
    let mut completions = column_types_simple();
    completions.extend(complex_type_snippets(Some(&string_byte_range)));
    completions
}

/// Full `type` completions for a bare value slot (no surrounding quotes).
/// Both simple strings and complex object snippets are valid.
pub(super) fn column_types_full() -> Vec<DomainCompletion> {
    let mut completions = COLUMN_TYPES
        .iter()
        .map(|column_type| {
            value_with_insert(
                column_type,
                format!("Column type: {column_type}"),
                format!("\"{column_type}\""),
            )
        })
        .collect::<Vec<_>>();

    completions.extend(complex_type_snippets(None));
    completions
}

fn complex_type_snippets(replace_range: Option<&std::ops::Range<usize>>) -> Vec<DomainCompletion> {
    [
        (
            "varchar(N)",
            "Variable-length string",
            r#"{"kind":"varchar","length":${1:255}}"#,
        ),
        (
            "char(N)",
            "Fixed-length string",
            r#"{"kind":"char","length":${1:2}}"#,
        ),
        (
            "numeric(P,S)",
            "Fixed-precision decimal",
            r#"{"kind":"numeric","precision":${1:10},"scale":${2:2}}"#,
        ),
        (
            "enum",
            "Native string enum",
            r#"{"kind":"enum","name":"${1:status}","values":["${2:active}","${3:inactive}"]}"#,
        ),
    ]
    .into_iter()
    .map(|(label, detail, insert_text)| DomainCompletion {
        label: label.to_string(),
        kind: CompletionItemKind::Snippet,
        detail: Some(detail.to_string()),
        insert_text: Some(insert_text.to_string()),
        sort_priority: 2,
        replace_range_bytes: replace_range.cloned(),
    })
    .collect()
}

/// `kind` candidates for a complex `type` object. When the cursor sits
/// inside a string literal, accepting a suggestion replaces just the
/// inner content so partial typing (e.g. `var`) is wiped while the
/// surrounding quotes stay intact. In a bare value slot the suggestion
/// inserts the full JSON-quoted form.
///
/// Critically the INNER range (without quotes) is used as the replace
/// range. Outer-quote ranges would force the LSP client to filter
/// completions against the leading `"`, which VS Code (correctly) rejects
/// because `varchar` does not match a `"` prefix — that is why this
/// completion appeared "broken" in VS Code but worked in Zed (Zed is
/// more lenient about filter prefixes).
pub(super) fn type_kind_values(
    string_byte_range: Option<&std::ops::Range<usize>>,
) -> Vec<DomainCompletion> {
    const KINDS: &[(&str, &str)] = &[
        ("varchar", "Variable-length string"),
        ("char", "Fixed-length string"),
        ("numeric", "Fixed-precision decimal"),
        ("enum", "Native enum"),
        ("custom", "Raw SQL type"),
    ];

    let inner_range = string_byte_range.map(outer_to_inner_range);
    let in_string = string_byte_range.is_some();

    KINDS
        .iter()
        .enumerate()
        .map(|(idx, (kind, detail))| {
            // Inside a string literal: replace only the contents
            // (`"v"` → `"varchar"`). Outside: emit a JSON-quoted literal
            // so the value slot becomes syntactically valid.
            let insert_text = if in_string {
                (*kind).to_string()
            } else {
                format!("\"{kind}\"")
            };
            DomainCompletion {
                label: (*kind).to_string(),
                kind: CompletionItemKind::Value,
                detail: Some((*detail).to_string()),
                insert_text: Some(insert_text),
                sort_priority: u8::try_from(idx + 1).unwrap_or(u8::MAX),
                replace_range_bytes: inner_range.clone(),
            }
        })
        .collect()
}

/// Strip one byte from each side of a JSON/YAML quoted scalar range so
/// the result covers only the text content. Falls back to the original
/// range when the literal is too short to have quotes (defensive).
fn outer_to_inner_range(range: &std::ops::Range<usize>) -> std::ops::Range<usize> {
    if range.end.saturating_sub(range.start) >= 2 {
        (range.start + 1)..(range.end - 1)
    } else {
        range.clone()
    }
}

pub(super) fn reference_actions() -> Vec<DomainCompletion> {
    REFERENCE_ACTIONS
        .iter()
        .map(|action| value(action, format!("Reference action: {action}")))
        .collect()
}

pub(super) fn check_expr_operands(columns: &[String]) -> Vec<DomainCompletion> {
    let mut completions = check_expr_column_completions(columns, None);
    completions.extend(check_expr_values(CHECK_EXPR_OPERAND_HELPERS));
    completions
}

pub(super) fn check_expr_operators() -> Vec<DomainCompletion> {
    check_expr_values(CHECK_EXPR_OPERATORS_AND_KEYWORDS)
}

pub(super) fn check_expr_partial_columns(
    columns: &[String],
    prefix: &str,
    replace_range: Option<&Range<usize>>,
) -> Vec<DomainCompletion> {
    let matching = columns
        .iter()
        .filter(|column| column.starts_with(prefix))
        .cloned()
        .collect::<Vec<_>>();
    check_expr_column_completions(&matching, replace_range)
}

fn check_expr_column_completions(
    columns: &[String],
    replace_range: Option<&Range<usize>>,
) -> Vec<DomainCompletion> {
    columns
        .iter()
        .enumerate()
        .map(|(idx, column)| DomainCompletion {
            label: column.clone(),
            kind: CompletionItemKind::Reference,
            detail: Some("CHECK expression column".to_string()),
            insert_text: replace_range.map(|_| column.clone()),
            sort_priority: u8::try_from(idx + 1).unwrap_or(u8::MAX),
            replace_range_bytes: replace_range.cloned(),
        })
        .collect()
}

fn check_expr_values(entries: &[(&str, &str)]) -> Vec<DomainCompletion> {
    entries
        .iter()
        .enumerate()
        .map(|(idx, (label, detail))| DomainCompletion {
            label: (*label).to_string(),
            kind: CompletionItemKind::Value,
            detail: Some((*detail).to_string()),
            sort_priority: u8::try_from(idx + 1).unwrap_or(u8::MAX),
            ..DomainCompletion::default()
        })
        .collect()
}

/// Default-value variants the LSP knows how to format.
#[derive(Debug, Clone, Copy)]
enum DefaultKind {
    /// Bare JSON literal (`null`, `true`, `false`, `0`, `1`, `-1`).
    /// Inside a quoted slot we still want the literal — quotes get wiped.
    JsonLiteral,
    /// SQL expression that must live inside a JSON string
    /// (`now()`, `gen_random_uuid()`, `'active'`, `''`).
    SqlExpression,
    /// Same as [`SqlExpression`] but carries snippet placeholders such as
    /// `'${1:value}'`. Emitted as `InsertTextFormat::SNIPPET`.
    SqlSnippet,
}

struct DefaultCandidate {
    /// Label shown in the popup, in canonical SQL form (without JSON quotes).
    label: String,
    detail: String,
    kind: DefaultKind,
}

/// Candidate values for `default`, tuned to the sibling `type`.
///
/// When the cursor is inside an existing `"..."` literal, every candidate
/// REPLACES that whole string node (quotes included) so the user never sees
/// the suggestion appended to leftover text.
pub(super) fn default_values(
    type_kind: Option<&str>,
    enum_values: &[String],
    string_byte_range: Option<&std::ops::Range<usize>>,
) -> Vec<DomainCompletion> {
    let mut candidates = type_candidates(type_kind, enum_values);
    // `null` is always valid as a SQL default.
    candidates.push(DefaultCandidate {
        label: "null".to_string(),
        detail: "SQL NULL".to_string(),
        kind: DefaultKind::JsonLiteral,
    });

    candidates
        .into_iter()
        .enumerate()
        .map(|(idx, candidate)| {
            let sort_priority = u8::try_from(idx + 1).unwrap_or(u8::MAX);
            build_default_completion(&candidate, string_byte_range, sort_priority)
        })
        .collect()
}

fn type_candidates(type_kind: Option<&str>, enum_values: &[String]) -> Vec<DefaultCandidate> {
    match type_kind {
        Some("enum") => enum_candidates(enum_values),
        Some("timestamp" | "timestamptz") => vec![
            sql_expr("now()", "Postgres: current timestamp"),
            sql_expr("CURRENT_TIMESTAMP", "ANSI SQL: current timestamp"),
            sql_expr("CURRENT_DATE", "ANSI SQL: current date"),
            sql_expr("CURRENT_TIME", "ANSI SQL: current time"),
        ],
        Some("date") => vec![sql_expr("CURRENT_DATE", "Current date")],
        Some("time") => vec![sql_expr("CURRENT_TIME", "Current time (no date)")],
        Some("uuid") => vec![
            sql_expr("gen_random_uuid()", "Postgres 13+ built-in"),
            sql_expr("uuid_generate_v4()", "Postgres uuid-ossp extension"),
        ],
        Some("boolean") => vec![
            json_literal("true", "Boolean true"),
            json_literal("false", "Boolean false"),
        ],
        Some("text" | "varchar" | "char") => vec![
            sql_expr("''", "Empty string literal"),
            DefaultCandidate {
                label: "'${1:value}'".to_string(),
                detail: "Custom string literal".to_string(),
                kind: DefaultKind::SqlSnippet,
            },
        ],
        Some("integer" | "big_int" | "small_int" | "numeric" | "real" | "double_precision") => {
            vec![
                json_literal("0", "Zero"),
                json_literal("1", "One"),
                json_literal("-1", "Negative one"),
            ]
        }
        Some("json" | "jsonb") => vec![
            sql_expr("'{}'", "Empty JSON object"),
            sql_expr("'[]'", "Empty JSON array"),
        ],
        // Unknown / partial type — surface a generous fallback so the
        // user always sees SOMETHING. Once the `type` value firms up the
        // list narrows to type-appropriate candidates only.
        _ => generic_default_fallback(),
    }
}

/// Catch-all default candidates for columns whose `type` could not be
/// statically resolved (typing in progress, custom type, etc.).
fn generic_default_fallback() -> Vec<DefaultCandidate> {
    vec![
        json_literal("0", "Zero"),
        json_literal("true", "Boolean true"),
        json_literal("false", "Boolean false"),
        sql_expr("''", "Empty string literal"),
        sql_expr("now()", "Current timestamp"),
        sql_expr("CURRENT_TIMESTAMP", "ANSI SQL: current timestamp"),
        sql_expr("gen_random_uuid()", "Random UUID v4"),
    ]
}

fn enum_candidates(enum_values: &[String]) -> Vec<DefaultCandidate> {
    enum_values
        .iter()
        .map(|name| DefaultCandidate {
            label: format!("'{name}'"),
            detail: format!("Enum value: {name}"),
            kind: DefaultKind::SqlExpression,
        })
        .collect()
}

fn sql_expr(literal: &'static str, detail: &'static str) -> DefaultCandidate {
    DefaultCandidate {
        label: literal.to_string(),
        detail: detail.to_string(),
        kind: DefaultKind::SqlExpression,
    }
}

fn json_literal(literal: &'static str, detail: &'static str) -> DefaultCandidate {
    DefaultCandidate {
        label: literal.to_string(),
        detail: detail.to_string(),
        kind: DefaultKind::JsonLiteral,
    }
}

/// Translate a [`DefaultCandidate`] into the final [`DomainCompletion`].
///
/// * When `string_byte_range` is `Some(range)` — the cursor sits inside a
///   `"..."` literal — only the INNER content is replaced. Surrounding
///   quotes stay in place; the inserted text fills them. This keeps VS
///   Code's prefix-filter happy (it rejects labels that do not match
///   against a leading `"` when the textEdit range starts on the quote).
/// * When `string_byte_range` is `None` — bare value slot — the completion
///   inserts at the cursor with an appropriately quoted form so the JSON
///   value is syntactically complete.
fn build_default_completion(
    candidate: &DefaultCandidate,
    string_byte_range: Option<&std::ops::Range<usize>>,
    sort_priority: u8,
) -> DomainCompletion {
    let in_string = string_byte_range.is_some();
    // Two cases:
    //   * Inside a string — the surrounding quotes already exist, so
    //     insert the raw text (`'active'`, `now()`, `null`). The user's
    //     `default` ends up as a JSON string wrapping a SQL expression /
    //     literal, which is exactly the schema's convention.
    //   * Bare value slot — JSON literals stay bare (`null`/`true`/`0`);
    //     SQL expressions get JSON-quoted so the slot remains valid.
    let insert_text = if in_string {
        candidate.label.clone()
    } else {
        match candidate.kind {
            DefaultKind::JsonLiteral => candidate.label.clone(),
            DefaultKind::SqlExpression | DefaultKind::SqlSnippet => {
                format!("\"{}\"", candidate.label)
            }
        }
    };
    let kind = match candidate.kind {
        DefaultKind::SqlSnippet => CompletionItemKind::Snippet,
        _ => CompletionItemKind::Value,
    };

    DomainCompletion {
        label: candidate.label.clone(),
        kind,
        detail: Some(candidate.detail.clone()),
        insert_text: Some(insert_text),
        sort_priority,
        // Inner-only range avoids VS Code's prefix-filter rejecting items
        // whose label does not start with `"`.
        replace_range_bytes: string_byte_range.map(outer_to_inner_range),
    }
}

pub(super) fn table_top_level_keys() -> Vec<DomainCompletion> {
    object_keys(TABLE_TOP_LEVEL_KEYS)
}

pub(super) fn column_object_keys() -> Vec<DomainCompletion> {
    object_keys(COLUMN_OBJECT_KEYS)
}

pub(super) fn foreign_key_object_keys() -> Vec<DomainCompletion> {
    object_keys(FOREIGN_KEY_OBJECT_KEYS)
}

pub(super) fn type_object_keys() -> Vec<DomainCompletion> {
    object_keys(TYPE_OBJECT_KEYS)
}

fn object_keys(entries: &[(&str, &str)]) -> Vec<DomainCompletion> {
    entries
        .iter()
        .enumerate()
        .map(|(idx, (label, detail))| DomainCompletion {
            label: (*label).to_string(),
            kind: CompletionItemKind::Property,
            detail: Some((*detail).to_string()),
            // Preserve the curated order via sort_priority.
            sort_priority: u8::try_from(idx + 1).unwrap_or(u8::MAX),
            ..DomainCompletion::default()
        })
        .collect()
}

pub(super) fn booleans() -> Vec<DomainCompletion> {
    ["true", "false"]
        .into_iter()
        .map(|label| DomainCompletion {
            label: label.to_string(),
            kind: CompletionItemKind::Value,
            sort_priority: 1,
            ..DomainCompletion::default()
        })
        .collect()
}

pub(super) fn tables_in_workspace(
    index: &WorkspaceIndex,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<DomainCompletion> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for name in index.tables() {
        seen.insert(name.clone());
        out.push(table_completion(name));
    }

    if let Some(disk_tables) = disk_tables {
        for name in disk_tables.names() {
            if seen.insert(name.clone()) {
                out.push(table_completion(name));
            }
        }
    }

    out
}

pub(super) fn columns_of(
    table_name: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<DomainCompletion> {
    if let Some(loc) = index.lookup(table_name) {
        let open_columns = docs
            .with_doc(&loc.uri, |text, _tree| {
                parse_table(text)
                    .map_or_else(Vec::new, |table| column_completions(table_name, &table))
            })
            .unwrap_or_default();

        if !open_columns.is_empty() {
            return open_columns;
        }
    }

    disk_tables
        .and_then(|tables| tables.get(table_name))
        .map_or_else(Vec::new, |table| column_completions(table_name, &table))
}

fn table_completion(name: String) -> DomainCompletion {
    DomainCompletion {
        detail: Some(format!("Table: {name}")),
        label: name,
        kind: CompletionItemKind::Reference,
        sort_priority: 1,
        ..DomainCompletion::default()
    }
}

fn column_completions(
    table_name: &str,
    table: &vespertide_core::TableDef,
) -> Vec<DomainCompletion> {
    table
        .columns
        .iter()
        .map(|column| DomainCompletion {
            label: column.name.as_str().to_string(),
            kind: CompletionItemKind::Reference,
            detail: Some(format!("Column in {table_name}")),
            sort_priority: 1,
            ..DomainCompletion::default()
        })
        .collect()
}

fn value(label: &str, detail: String) -> DomainCompletion {
    DomainCompletion {
        label: label.to_string(),
        kind: CompletionItemKind::Value,
        detail: Some(detail),
        sort_priority: 1,
        ..DomainCompletion::default()
    }
}

fn value_with_insert(label: &str, detail: String, insert_text: String) -> DomainCompletion {
    DomainCompletion {
        label: label.to_string(),
        kind: CompletionItemKind::Value,
        detail: Some(detail),
        insert_text: Some(insert_text),
        sort_priority: 1,
        ..DomainCompletion::default()
    }
}

fn parse_table(text: &str) -> Option<vespertide_core::TableDef> {
    serde_json::from_str(text)
        .ok()
        .or_else(|| serde_yaml::from_str(text).ok())
}

#[cfg(test)]
mod tests {
    use super::super::completion_labels;
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::complex_enum(Some("enum"), &["alpha", "beta"], &["'alpha'", "'beta'"], &["now()"])]
    #[case::unknown_complex_kind(Some("custom"), &[], &["null"], &[])]
    #[case::jsonb(Some("jsonb"), &[], &["'{}'", "'[]'"], &[])]
    #[case::date(Some("date"), &[], &["CURRENT_DATE"], &[])]
    #[case::time(Some("time"), &[], &["CURRENT_TIME"], &[])]
    #[case::text(Some("text"), &[], &["''", "'${1:value}'"], &[])]
    #[case::integer_enum_member_names(Some("enum"), &["low", "high"], &["'low'", "'high'"], &["now()"])]
    fn default_value_candidate_cases(
        #[case] type_kind: Option<&str>,
        #[case] enum_values: &[&str],
        #[case] expected_present: &[&str],
        #[case] expected_absent: &[&str],
    ) {
        let enum_values = enum_values
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let items = default_values(type_kind, &enum_values, None);
        let labels = completion_labels(&items);

        for &expected in expected_present {
            assert!(
                labels.contains(&expected),
                "expected `{expected}` in labels: {labels:?}"
            );
        }
        for &unexpected in expected_absent {
            assert!(
                !labels.contains(&unexpected),
                "unexpected `{unexpected}` in labels: {labels:?}"
            );
        }
    }

    #[test]
    fn type_kind_value_at_bare_slot_carries_quoted_insert_text() {
        let items = type_kind_values(None);
        let varchar = items.iter().find(|item| item.label == "varchar").unwrap();

        assert!(varchar.replace_range_bytes.is_none());
        assert_eq!(varchar.insert_text.as_deref(), Some("\"varchar\""));
    }

    #[test]
    fn numeric_default_candidates_are_json_literals() {
        let items = default_values(Some("integer"), &[], None);
        let labels = completion_labels(&items);

        assert!(labels.contains(&"0"));
        assert!(labels.contains(&"1"));
        assert!(labels.contains(&"-1"));
    }

    #[test]
    fn sql_default_candidates_are_quoted_outside_strings() {
        let items = default_values(Some("timestamp"), &[], None);
        let now = items
            .iter()
            .find(|item| item.label == "now()")
            .expect("now completion");

        assert_eq!(now.insert_text.as_deref(), Some("\"now()\""));
    }

    #[test]
    fn sql_default_candidates_replace_inner_range_inside_strings() {
        let string_range = 10..12;
        let items = default_values(Some("text"), &[], Some(&string_range));
        let snippet = items
            .iter()
            .find(|item| item.label == "'${1:value}'")
            .expect("snippet completion");

        assert_eq!(snippet.kind, CompletionItemKind::Snippet);
        assert_eq!(snippet.insert_text.as_deref(), Some("'${1:value}'"));
        assert_eq!(snippet.replace_range_bytes, Some(11..11));
    }
}
