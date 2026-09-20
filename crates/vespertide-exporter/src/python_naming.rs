//! Shared `to_pascal_case`: split on `_`, upper-case the first character of
//! each segment, keep the rest verbatim. SQLAlchemy, SQLModel, JPA, Django,
//! GORM and the CLI's filename derivation all want exactly that rule, which
//! is a naming convention rather than a language feature — which is why the
//! Java and Go backends share it instead of carrying copies.
//!
//! Enum member names go through `vespertide_naming::to_screaming_snake_case` +
//! `sanitize_identifier` instead — that pair is shared with the Prisma backend,
//! so the case rule lives in `vespertide-naming` rather than here.
//!
//! `seaorm` keeps its own `to_pascal_case` in `seaorm/imports.rs`: that variant
//! also treats `-` as a separator and upper-cases with `to_ascii_uppercase`
//! rather than Unicode-aware `char::to_uppercase`, so the two are not
//! interchangeable. Reserved-keyword escaping is a separate concern, handled
//! by `seaorm::imports::sanitize_field_name`.

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
