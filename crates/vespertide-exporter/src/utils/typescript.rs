//! TypeScript-specific naming helpers.
//!
//! Shared by every exporter whose output is TypeScript source (currently
//! Drizzle). Anything that is not tied to TypeScript belongs in
//! `vespertide-naming` instead.

use vespertide_naming::{IdentifierStart, sanitize_identifier};

/// Words that may not name a `const` binding: the ECMAScript reserved words
/// plus the strict-mode set, which a generated module is always subject to.
/// `await` is included because module code is an implicit async context.
///
/// Object keys would not strictly need this rewrite — a reserved word is legal
/// both as a key (`{ class: … }`) and after a dot (`posts.class`) — but keys
/// and bindings share one naming rule anyway: a table's const, its column
/// keys, and every `t.…` / relations reference must resolve to the same
/// identifier, so escaping only some of them would tear those apart.
const RESERVED_BINDINGS: &[&str] = &[
    // Not reserved words, but strict mode — which module code always is —
    // rejects either as a binding name outright.
    "arguments",
    "eval",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Rewrite `name` into an identifier usable as a TypeScript `const` binding.
///
/// Runs [`sanitize_identifier`] with the letter rule — TypeScript would accept
/// a leading `_`, but keeping the rule the other letter-start backends use
/// means a digit-leading table reads the same across generated output — and
/// then appends `_` to anything colliding with a reserved word.
///
/// The database name is never carried by the result, so a caller **must** emit
/// the original alongside it. In Drizzle that is free: the name is already the
/// first argument of every constructor (`pgTable("class", …)`).
pub(crate) fn ts_binding(name: &str) -> String {
    let sanitized = sanitize_identifier(name, IdentifierStart::Letter);
    if RESERVED_BINDINGS.contains(&sanitized.as_str()) {
        return sanitized + "_";
    }
    sanitized
}

/// Quote `value` as a double-quoted TypeScript string literal.
///
/// Backslashes, quotes and the line terminators are escaped so a database name
/// or enum value containing any of them cannot end the literal early. Every
/// literal Drizzle emits — table names, column names, enum values — goes
/// through here.
pub(crate) fn ts_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::plain("users", "users")]
    #[case::leading_digit("1users", "x1users")]
    #[case::hyphen("user-id", "user_id")]
    #[case::reserved("class", "class_")]
    #[case::reserved_default("default", "default_")]
    #[case::reserved_new("new", "new_")]
    #[case::strict_restricted_eval("eval", "eval_")]
    #[case::strict_restricted_arguments("arguments", "arguments_")]
    // `order` and `select` are reserved in SQL, not in TypeScript.
    #[case::sql_reserved_only("order", "order")]
    fn ts_binding_escapes_digits_and_reserved_words(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(ts_binding(input), expected);
    }

    #[rstest]
    #[case::plain("users", r#""users""#)]
    #[case::double_quote("say \"hi\"", r#""say \"hi\"""#)]
    #[case::backslash("back\\slash", r#""back\\slash""#)]
    #[case::newline("two\nlines", r#""two\nlines""#)]
    #[case::carriage_return("a\rb", r#""a\rb""#)]
    #[case::tab("a\tb", r#""a\tb""#)]
    #[case::empty("", r#""""#)]
    fn ts_string_escapes_literal_terminators(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(ts_string(input), expected);
    }
}
