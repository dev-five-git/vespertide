use std::collections::{HashMap, HashSet};

use vespertide_naming::{IdentifierStart, seaorm_module_name};

/// Build an absolute `crate::` module path for the target table.
///
/// `crate_prefix` is derived from the export directory (e.g., `"src/models"` → `"crate::models"`).
/// `to_module` is the module path segments of the target table (e.g., `["admin", "admin"]`).
///
/// Returns a path like `crate::models::admin::admin`.
pub(super) fn absolute_module_path(crate_prefix: &str, to_module: &[String]) -> String {
    // Pre-size to the exact final length (`crate_prefix` + `"::" + seg` per
    // segment) so multi-segment paths allocate once instead of climbing the
    // doubling ladder. Output byte-identical.
    let cap = crate_prefix.len() + to_module.iter().map(|s| s.len() + 2).sum::<usize>();
    let mut path = String::with_capacity(cap);
    path.push_str(crate_prefix);
    for seg in to_module {
        path.push_str("::");
        path.push_str(seg);
    }
    path
}

/// Resolve relation field entity paths for `SeaORM` model macros.
///
/// Rule:
/// - same folder → `super::{table}`
/// - different folder → absolute `crate::...` path
///
/// This avoids generating brittle `super::super::...` paths for cross-folder relations.
pub(super) fn resolve_relation_entity_module_path(
    current_table: &str,
    target_table: &str,
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> String {
    if let (Some(current), Some(target)) = (
        module_paths.get(current_table),
        module_paths.get(target_table),
    ) {
        let current_parent = current.split_last().map_or(&[][..], |(_, parent)| parent);
        let target_parent = target.split_last().map_or(&[][..], |(_, parent)| parent);

        if current_parent == target_parent {
            return format!("super::{}", seaorm_module_name(target_table));
        }

        if !crate_prefix.is_empty() {
            return absolute_module_path(crate_prefix, target);
        }

        return format!("super::{}", seaorm_module_name(target_table));
    }

    if !crate_prefix.is_empty() {
        return format!("{crate_prefix}::{}", seaorm_module_name(target_table));
    }

    format!("super::{}", seaorm_module_name(target_table))
}
/// Rust reserved keywords that cannot be used as identifiers without raw identifier syntax.
/// Reference: <https://doc.rust-lang.org/reference/keywords.html>
pub(super) const RUST_KEYWORDS: &[&str] = &[
    // Strict keywords
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", // Reserved keywords (for future use)
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try", "typeof",
    "unsized", "virtual", "yield",
];

/// Field name for a `SeaORM` model.
///
/// `DeriveEntityModel` turns each field name into a `Column` enum variant by
/// `PascalCase`-ing it, which drops a leading `_`: `_1st_place` becomes
/// `1stPlace` and the derive macro panics. Only a digit after the underscore
/// actually breaks it (`_name` is fine), but escaping with a letter throughout
/// keeps one rule instead of a special case.
pub(super) fn sanitize_field_name(name: &str) -> String {
    if name.is_empty() {
        return "_col".into();
    }

    let mut result = vespertide_naming::sanitize_identifier(name, IdentifierStart::Letter);

    if RUST_KEYWORDS.contains(&result.as_str()) {
        // Reuse the already-allocated `result` buffer instead of allocating a
        // second `String` via `format!`; the 2-byte `"r#"` prefix is a single
        // memmove. Output is byte-identical to `format!("r#{result}")`.
        result.insert_str(0, "r#");
    }
    result
}

/// Name for a `SeaORM` relation enum variant or `Linked` struct.
///
/// `sea-orm` turns `relation_enum` / `via_rel` strings into an `Ident`
/// verbatim, so a `PascalCase` name built from a table or column still has to
/// be a legal identifier. The letter rule matches [`sanitize_field_name`], so
/// the whole backend escapes the same way.
pub(super) fn sanitize_type_name(name: &str) -> String {
    let mut result = vespertide_naming::sanitize_identifier(name, IdentifierStart::Letter);
    // The escape letter takes the case of the name it precedes, which reads
    // wrong on a type: `x1stOwner` compiles but trips `non_camel_case_types`.
    if let Some(first) = result.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    result
}

pub(super) fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
    let mut name = base.to_string();
    let mut i = 1;
    while used.contains(&name) {
        name = format!("{base}_{i}");
        i += 1;
    }
    used.insert(name.clone());
    name
}
pub(super) fn to_pascal_case(s: &str) -> String {
    // Separators are dropped, so the output never exceeds `s.len()`.
    let mut result = String::with_capacity(s.len());
    let mut capitalize = true;
    for c in s.chars() {
        let is_separator = c == '_' || c == '-';
        if is_separator {
            capitalize = true;
            continue;
        }
        let ch = if capitalize {
            c.to_ascii_uppercase()
        } else {
            c
        };
        capitalize = false;
        result.push(ch);
    }
    result
}

/// Convert `PascalCase` to `snake_case`.
/// For "`CreatorUser`", generates "`creator_user`".
pub(super) fn to_snake_case(s: &str) -> String {
    // Each uppercase run inserts one `_`; a small slack over `s.len()` avoids a
    // realloc for typical PascalCase input without over-sizing.
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}
