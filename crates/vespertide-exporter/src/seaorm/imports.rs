use std::collections::{HashMap, HashSet};

/// Build an absolute `crate::` module path for the target table.
///
/// `crate_prefix` is derived from the export directory (e.g., `"src/models"` → `"crate::models"`).
/// `to_module` is the module path segments of the target table (e.g., `["admin", "admin"]`).
///
/// Returns a path like `crate::models::admin::admin`.
pub(super) fn absolute_module_path(crate_prefix: &str, to_module: &[String]) -> String {
    let mut path = crate_prefix.to_string();
    for seg in to_module {
        path.push_str("::");
        path.push_str(seg);
    }
    path
}

/// Look up the module path for a table name from the `module_paths` map.
/// Uses `super::` for sibling modules in the same folder, `crate::` absolute paths for
/// cross-directory relations when mappings are available, and falls back to `super::{table_name}`.
#[cfg(test)]
pub(super) fn resolve_entity_module_path(
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
            return format!("super::{target_table}");
        }

        if !crate_prefix.is_empty() {
            return absolute_module_path(crate_prefix, target);
        }
    }

    format!("super::{target_table}")
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
            return format!("super::{target_table}");
        }

        if !crate_prefix.is_empty() {
            return absolute_module_path(crate_prefix, target);
        }

        return format!("super::{target_table}");
    }

    if !crate_prefix.is_empty() {
        return format!("{crate_prefix}::{target_table}");
    }

    format!("super::{target_table}")
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

pub(super) fn sanitize_field_name(name: &str) -> String {
    let mut result = String::new();

    for (idx, ch) in name.chars().enumerate() {
        if (ch.is_ascii_alphanumeric() && (idx > 0 || ch.is_ascii_alphabetic())) || ch == '_' {
            result.push(ch);
        } else if idx == 0 && ch.is_ascii_digit() {
            result.push('_');
            result.push(ch);
        } else {
            result.push('_');
        }
    }

    if result.is_empty() {
        "_col".into()
    } else if RUST_KEYWORDS.contains(&result.as_str()) {
        format!("r#{result}")
    } else {
        result
    }
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
    let mut result = String::new();
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
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}
