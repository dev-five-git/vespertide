//! Shared naming helpers for the Python-targeted ORM exporters (SQLAlchemy,
//! SQLModel). Both backends share an identical, snake-case-aware
//! `to_pascal_case`.
//!
//! Enum member names go through `vespertide_naming::to_screaming_snake_case` +
//! `sanitize_identifier` instead — that pair is shared with the Prisma backend,
//! so the case rule lives in `vespertide-naming` rather than here.
//!
//! `seaorm` deliberately keeps its own `to_pascal_case` in
//! `seaorm/imports.rs` — that variant carries reserved-keyword guards and a
//! different allocation pattern and is NOT in scope for this consolidation.

/// Convert snake_case (or single-word) input to PascalCase. Splits on
/// underscores, upper-cases the first character of each segment, and
/// preserves the remainder verbatim.
///
/// Public so the `vespertide-cli` `export` command can reuse the exact same
/// PascalCase semantics for JPA filename derivation without keeping a
/// duplicate private implementation.
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for word in s.split('_') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.push_str(chars.as_str());
        }
    }
    result
}
