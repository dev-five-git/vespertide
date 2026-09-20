mod enums;
mod render;
mod types;

use std::path::Path;

use crate::orm::OrmExporter;
use crate::scope_names::scope_of;
use render::{gofmt_layout, imports_for, package_names, render_header, render_table_body};
use vespertide_core::TableDef;

pub struct GormExporter;

impl OrmExporter for GormExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema(table, schema)
    }
}

/// Go package name for renders that have no export directory to derive one
/// from, and the fallback when the directory's name does not yield a usable
/// Go identifier.
const DEFAULT_GORM_PACKAGE_NAME: &str = "models";

/// Go reserved words, which can't be used as a package name.
const GO_RESERVED_WORDS: &[&str] = &[
    "break",
    "default",
    "func",
    "interface",
    "select",
    "case",
    "defer",
    "go",
    "map",
    "struct",
    "chan",
    "else",
    "goto",
    "package",
    "switch",
    "const",
    "fallthrough",
    "if",
    "range",
    "type",
    "continue",
    "for",
    "import",
    "return",
    "var",
];

/// Sanitize a candidate string into a valid, idiomatic Go package identifier:
/// lowercase ASCII letters/digits only, must not start with a digit, must
/// not collide with a Go reserved word. Returns `None` when nothing usable
/// remains (e.g. an all-Unicode or empty candidate).
fn sanitize_go_package_name(candidate: &str) -> Option<String> {
    let cleaned: String = candidate
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();

    if cleaned.is_empty() || cleaned.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if GO_RESERVED_WORDS.contains(&cleaned.as_str()) {
        return None;
    }
    Some(cleaned)
}

/// Go package name for a GORM export: the export directory's final path
/// segment, sanitized into a Go identifier, or [`DEFAULT_GORM_PACKAGE_NAME`]
/// when that segment yields nothing usable.
fn go_package_name(export_dir: &Path) -> String {
    export_dir
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(sanitize_go_package_name)
        .unwrap_or_else(|| DEFAULT_GORM_PACKAGE_NAME.to_string())
}

/// GORM exporter whose `package` clause names the directory the file is
/// written to, which is what Go expects of it.
pub struct GormExporterWithConfig {
    package_name: String,
}

impl GormExporterWithConfig {
    /// `export_dir` is the directory `models.go` is actually written to —
    /// whatever wins after resolving `--export-dir`.
    pub fn for_export_dir(export_dir: &Path) -> Self {
        Self {
            package_name: go_package_name(export_dir),
        }
    }

    /// [`export`] under the directory's package name.
    pub fn export(&self, schema: &[TableDef]) -> Result<String, String> {
        Ok(export_with_package(schema, &self.package_name))
    }
}

/// Render a GORM entity for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    Ok(render_entity_inner(table, &[]))
}

/// Render a GORM entity with full schema context, which the reverse side of
/// every relation (has-one / has-many) needs.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    Ok(render_entity_inner(table, schema))
}

fn render_entity_inner(table: &TableDef, schema: &[TableDef]) -> String {
    let mut lines = render_header(
        DEFAULT_GORM_PACKAGE_NAME,
        &imports_for(std::slice::from_ref(table)),
    );
    let names = package_names(scope_of(table, schema));
    lines.extend(render_table_body(table, schema, &names));
    gofmt_layout(&lines)
}

/// Render a whole schema as one Go source file: a single `package` clause,
/// one import block covering every table, then each table's declarations.
/// Concatenating per-table files instead would repeat the `package` clause,
/// which Go rejects.
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    Ok(export_with_package(schema, DEFAULT_GORM_PACKAGE_NAME))
}

fn export_with_package(schema: &[TableDef], package_name: &str) -> String {
    let mut lines = render_header(package_name, &imports_for(schema));
    let names = package_names(schema);
    for (i, table) in schema.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        lines.extend(render_table_body(table, schema, &names));
    }
    gofmt_layout(&lines)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;

    use super::go_package_name;

    #[rstest]
    #[case::default_dir_matches_folder("src/models", "models")]
    #[case::infers_from_folder_name("src/entities", "entities")]
    #[case::strips_invalid_chars("src/db-models", "dbmodels")]
    #[case::falls_back_when_digit_led("src/2024-models", "models")]
    #[case::falls_back_on_non_ascii("src/모델", "models")]
    #[case::falls_back_on_reserved_word("src/type", "models")]
    fn go_package_name_inferred_from_export_dir(#[case] export_dir: &str, #[case] expected: &str) {
        assert_eq!(go_package_name(Path::new(export_dir)), expected);
    }
}
