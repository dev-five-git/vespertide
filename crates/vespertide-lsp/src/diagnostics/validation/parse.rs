use std::sync::OnceLock;

use vespertide_core::TableDef;

use crate::diagnostics::{DomainDiagnostic, Severity};

use super::byte_offset_for_line_col;
use super::cache::{CachedParseError, CachedParseResult, ParseCache};

static JSON_PARSE_CACHE: OnceLock<ParseCache> = OnceLock::new();
static YAML_PARSE_CACHE: OnceLock<ParseCache> = OnceLock::new();

fn json_parse_cache() -> &'static ParseCache {
    JSON_PARSE_CACHE.get_or_init(ParseCache::new)
}

fn yaml_parse_cache() -> &'static ParseCache {
    YAML_PARSE_CACHE.get_or_init(ParseCache::new)
}

pub(in crate::diagnostics) fn try_parse_json(
    text: &str,
    out: &mut Vec<DomainDiagnostic>,
) -> Option<TableDef> {
    let cached = json_parse_cache().get_or_parse(text, || parse_json_table(text));
    emit_cached_parse(cached.as_ref(), out)
}

pub(in crate::diagnostics) fn try_parse_yaml(
    text: &str,
    out: &mut Vec<DomainDiagnostic>,
) -> Option<TableDef> {
    let cached = yaml_parse_cache().get_or_parse(text, || parse_yaml_table(text));
    emit_cached_parse(cached.as_ref(), out)
}

fn parse_json_table(text: &str) -> CachedParseResult {
    match serde_json::from_str::<TableDef>(text) {
        Ok(table) => normalize_table(&table),
        Err(e) => {
            let byte = byte_offset_for_line_col(text, e.line(), e.column());
            Err(CachedParseError::new(
                byte..(byte + 1).min(text.len()),
                format!("JSON parse error: {e}"),
                "parse-error",
            ))
        }
    }
}

fn parse_yaml_table(text: &str) -> CachedParseResult {
    match serde_yaml::from_str::<TableDef>(text) {
        Ok(table) => normalize_table(&table),
        Err(e) => {
            let byte = e.location().map_or(0, |loc| loc.index().min(text.len()));
            Err(CachedParseError::new(
                byte..(byte + 1).min(text.len()),
                format!("YAML parse error: {e}"),
                "parse-error",
            ))
        }
    }
}

fn emit_cached_parse(
    cached: &CachedParseResult,
    out: &mut Vec<DomainDiagnostic>,
) -> Option<TableDef> {
    match cached {
        Ok(table) => Some(table.clone()),
        Err(error) => {
            push_cached_error(error, out);
            None
        }
    }
}

fn push_cached_error(error: &CachedParseError, out: &mut Vec<DomainDiagnostic>) {
    out.push(DomainDiagnostic {
        byte_range: error.byte_range.clone(),
        severity: Severity::Error,
        message: error.message.clone(),
        code: error.code.to_string(),
    });
}

/// Run `TableDef::normalize()` so inline constraints participate in planner validation.
fn normalize_table(table: &TableDef) -> CachedParseResult {
    table
        .normalize()
        .map_err(|e| CachedParseError::new(0..1, e.to_string(), "validate-schema"))
}
