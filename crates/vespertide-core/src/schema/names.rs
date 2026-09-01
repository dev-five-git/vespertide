//! Newtype wrappers for schema identifiers (tables, columns, indexes).
//!
//! These wrap `String` to provide compile-time type safety: a function
//! taking `TableName` cannot accidentally receive a `ColumnName`. Wire
//! format is preserved exactly via `#[serde(transparent)]` — JSON
//! migration scripts, schema files, and CLI output deserialize/serialize
//! byte-identically with the previous String-alias version.
//!
//! Convention: always `snake_case`, enforced by CLI / planner naming
//! validation rather than by the type system.

use std::fmt;

/// The name of a database table, always in `snake_case` by convention.
///
/// Construction:
///
/// ```rust
/// use vespertide_core::schema::names::TableName;
///
/// let via_new: TableName = TableName::new("user");
/// let via_from: TableName = "user".into();
///
/// assert_eq!(via_new.as_str(), "user");
/// assert!(via_new == "user");
/// assert_eq!(via_new.to_string(), "user");
/// assert_eq!(via_new, via_from);
/// ```
///
/// JSON wire format is byte-identical to a plain `String` thanks to
/// `#[serde(transparent)]`. See [`ColumnName`] for a serde round-trip example.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct TableName(String);

/// The name of a table column, always in `snake_case` by convention.
///
/// Construction and serde round-trip:
///
/// ```rust
/// use vespertide_core::schema::names::ColumnName;
///
/// let col: ColumnName = ColumnName::new("email");
/// let via_from: ColumnName = "email".into();
///
/// assert_eq!(col.as_str(), "email");
/// assert!(col == "email");
/// assert_eq!(col, via_from);
///
/// // Wire format is byte-identical to a plain JSON string.
/// let json = serde_json::to_string(&col).unwrap();
/// assert_eq!(json, r#""email""#);
/// let back: ColumnName = serde_json::from_str(&json).unwrap();
/// assert_eq!(back, col);
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct ColumnName(String);

/// The name of a database index, conventionally `ix_{table}__{columns}`.
///
/// Construction:
///
/// ```rust
/// use vespertide_core::schema::names::IndexName;
///
/// let idx: IndexName = IndexName::new("ix_user__email");
/// let via_from: IndexName = "ix_user__email".into();
///
/// assert_eq!(idx.as_str(), "ix_user__email");
/// assert!(idx == "ix_user__email");
/// assert_eq!(idx.to_string(), "ix_user__email");
/// assert_eq!(idx, via_from);
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct IndexName(String);

// Implement common ergonomics. Each newtype gets the same impl block via a
// declarative macro to avoid 60 lines of triplication.
macro_rules! impl_name_newtype {
    ($ty:ident) => {
        impl $ty {
            #[must_use]
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $ty {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $ty {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<$ty> for String {
            fn from(t: $ty) -> Self {
                t.0
            }
        }

        impl From<&$ty> for String {
            fn from(t: &$ty) -> Self {
                t.0.clone()
            }
        }

        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::ops::Deref for $ty {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(&self.0, f)
            }
        }

        impl std::borrow::Borrow<str> for $ty {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $ty {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $ty {
            fn eq(&self, other: &&str) -> bool {
                &self.0 == *other
            }
        }

        impl PartialEq<String> for $ty {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }
    };
}

impl_name_newtype!(TableName);
impl_name_newtype!(ColumnName);
impl_name_newtype!(IndexName);

impl TableName {
    /// Prepend `prefix` to this table name in place, reusing the existing
    /// `String` allocation when capacity allows. No-op when
    /// `prefix.is_empty()`.
    ///
    /// Unifies the two pre-existing prepend patterns in
    /// `vespertide-core` (`action/prefix.rs`'s private `add_prefix` and
    /// `schema/constraint.rs`'s open-coded
    /// `format!("{prefix}{table}").into()`) — every "prefix a `TableName`"
    /// site now reads the same way and avoids the fresh `format!`
    /// allocation the latter shape always paid.
    ///
    /// ```rust
    /// use vespertide_core::schema::names::TableName;
    ///
    /// let prefixed = TableName::new("user").with_prefix("tenant_");
    /// assert_eq!(prefixed.as_str(), "tenant_user");
    ///
    /// // Empty prefix is a pure no-op.
    /// let unchanged = TableName::new("user").with_prefix("");
    /// assert_eq!(unchanged.as_str(), "user");
    /// ```
    #[must_use]
    pub fn with_prefix(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }
        let mut s = self.0;
        s.insert_str(0, prefix);
        Self(s)
    }
}

/// Convert any slice of stringy items (typically a `Vec<ColumnName>`) into
/// a `Vec<String>` — hoisted out of 14+ verbatim
/// `xxx.iter().map(ToString::to_string).collect()` chains across the planner
/// and query crates.
///
/// The generic bound `T: ToString` keeps the helper usable with both the
/// 0.2.0 [`ColumnName`] newtype (where `Display` produces the bare
/// identifier) and the older `&str` / `String` aliases that still sit
/// behind a few `Vec<&&str>` collect call sites.
///
/// ```rust
/// use vespertide_core::schema::names::{ColumnName, names_to_strings};
///
/// let cols: Vec<ColumnName> = vec!["a".into(), "b".into()];
/// assert_eq!(names_to_strings(&cols), vec!["a".to_string(), "b".to_string()]);
///
/// // Works with `&str` slices too — same `ToString` bound, no allocation
/// // change versus the inline pattern.
/// let strs = ["x", "y", "z"];
/// assert_eq!(names_to_strings(&strs), vec!["x".to_string(), "y".to_string(), "z".to_string()]);
///
/// // Empty slice is the canonical empty `Vec<String>`.
/// let empty: Vec<ColumnName> = vec![];
/// assert_eq!(names_to_strings(&empty), Vec::<String>::new());
/// ```
#[must_use]
pub fn names_to_strings<T: ToString>(names: &[T]) -> Vec<String> {
    names.iter().map(ToString::to_string).collect()
}

/// Join a slice of [`ColumnName`]s with a separator using a single buffer
/// — no intermediate `Vec<String>` allocation.
///
/// Mirrors the buffer-push pattern used by
/// `vespertide_query::sql::helpers::quote_idents`. Folds four open-coded
/// `cols.iter().map(...).collect::<Vec<_>>().join(sep)` callsites in
/// `vespertide-planner::validate::*` into a single shared helper.
///
/// ```rust
/// use vespertide_core::schema::names::{ColumnName, join_column_names};
///
/// let cols: Vec<ColumnName> = vec!["a".into(), "b".into(), "c".into()];
/// assert_eq!(join_column_names(&cols, ", "), "a, b, c");
/// assert_eq!(join_column_names(&cols, "_"), "a_b_c");
/// assert_eq!(join_column_names(&[], ", "), "");
/// ```
#[must_use]
pub fn join_column_names(columns: &[ColumnName], separator: &str) -> String {
    // Pre-size exactly: sum of column-name lengths plus one separator between
    // each adjacent pair. Finishes matching the buffer-push pattern this
    // helper's doc-comment claims to follow — `quote_ident` already pre-sizes
    // via `out.reserve(name.len() + 2)`. Output stays byte-identical.
    let cap = columns.iter().map(|c| c.as_str().len()).sum::<usize>()
        + separator.len() * columns.len().saturating_sub(1);
    let mut out = String::with_capacity(cap);
    for (i, c) in columns.iter().enumerate() {
        if i > 0 {
            out.push_str(separator);
        }
        out.push_str(c.as_str());
    }
    out
}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests for the `impl_name_newtype!` expansions.
    //! Tarpaulin attributes hits at the macro definition lines (91, 92 for
    //! `new`, 119, 120 for `From<$ty> for String`). Doctests do not run
    //! under tarpaulin, so we exercise the same paths from real `#[test]`s.
    use super::*;

    #[test]
    fn table_name_new_constructs_from_str_literal() {
        // Covers lines 91, 92 via TableName::new.
        let name = TableName::new("user");
        assert_eq!(name.as_str(), "user");
    }

    #[test]
    fn column_name_new_constructs_from_owned_string() {
        // Covers lines 91, 92 via ColumnName::new (different newtype).
        let name = ColumnName::new(String::from("email"));
        assert_eq!(name.as_str(), "email");
    }

    #[test]
    fn index_name_new_constructs_from_str_ref() {
        // Covers lines 91, 92 via IndexName::new.
        let owned = "ix_user__email".to_string();
        let name = IndexName::new(&*owned);
        assert_eq!(name.as_str(), "ix_user__email");
    }

    #[test]
    fn table_name_into_string_via_from() {
        // Covers lines 119, 120 (`From<TableName> for String`).
        let name = TableName::new("orders");
        let s: String = String::from(name);
        assert_eq!(s, "orders");
    }

    #[test]
    fn column_name_into_string_via_from() {
        // Covers lines 119, 120 (`From<ColumnName> for String`).
        let name = ColumnName::new("created_at");
        let s: String = String::from(name);
        assert_eq!(s, "created_at");
    }

    #[test]
    fn index_name_into_string_via_from() {
        // Covers lines 119, 120 (`From<IndexName> for String`).
        let name = IndexName::new("ix_orders__id");
        let s: String = String::from(name);
        assert_eq!(s, "ix_orders__id");
    }

    #[test]
    fn into_inner_consumes_newtype_back_into_string() {
        // Covers the `into_inner` macro-expansion lines. `String::from` goes
        // through the separate `From<$ty> for String` impl, so the tests above
        // do not reach this body; call it explicitly on each newtype.
        assert_eq!(TableName::new("user").into_inner(), "user");
        assert_eq!(ColumnName::new("email").into_inner(), "email");
        assert_eq!(
            IndexName::new("ix_user__email").into_inner(),
            "ix_user__email"
        );
    }

    #[test]
    fn with_prefix_empty_is_a_no_op() {
        // Covers the `prefix.is_empty()` early return in `TableName::with_prefix`.
        // Only the doctest exercised it, and doctests do not run under tarpaulin.
        let unchanged = TableName::new("user").with_prefix("");
        assert_eq!(unchanged.as_str(), "user");
    }

    #[test]
    fn with_prefix_prepends_in_place() {
        // Sibling of the empty case: the non-empty branch must actually prepend.
        let prefixed = TableName::new("user").with_prefix("tenant_");
        assert_eq!(prefixed.as_str(), "tenant_user");
    }

    #[test]
    fn join_column_names_empty_returns_empty_string() {
        // Empty-slice path: `for ... in []` never runs, returns the
        // pristine `String::new()`. Locks the empty-input contract that
        // the deleted `validate::constraint_drops::join_columns` helper
        // used to assert directly.
        let cols: Vec<ColumnName> = vec![];
        assert_eq!(join_column_names(&cols, ", "), "");
        assert_eq!(join_column_names(&cols, "_"), "");
    }

    #[test]
    fn join_column_names_comma_separator() {
        // The validate-diagnostic site shape: `"a, b, c"`. Single-column
        // case must skip the separator entirely (no leading `", "`).
        let cols: Vec<ColumnName> = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(join_column_names(&cols, ", "), "a, b, c");

        let single: Vec<ColumnName> = vec!["only".into()];
        assert_eq!(join_column_names(&single, ", "), "only");
    }

    #[test]
    fn join_column_names_underscore_separator() {
        // The `ix_{table}__{cols}` index-name builder shape: `"fk_id"`.
        // Locks the separator variation against the validate
        // `build_suggested_index_name` callsite.
        let cols: Vec<ColumnName> = vec!["fk".into(), "id".into()];
        assert_eq!(join_column_names(&cols, "_"), "fk_id");

        let composite: Vec<ColumnName> = vec!["tenant_id".into(), "user_id".into()];
        assert_eq!(join_column_names(&composite, "_"), "tenant_id_user_id");
    }

    /// `names_to_strings` empty-slice produces an empty `Vec<String>`. Locks
    /// the same empty-input contract previously baked into the inline
    /// `xxx.iter().map(ToString::to_string).collect()` form at every replaced
    /// callsite.
    #[test]
    fn names_to_strings_empty_returns_empty_vec() {
        let cols: Vec<ColumnName> = vec![];
        assert_eq!(names_to_strings(&cols), Vec::<String>::new());
    }

    /// `names_to_strings` on a populated slice produces the canonical
    /// per-element `to_string()` output in source order — the contract every
    /// planner / query crate call site silently depended on.
    #[test]
    fn names_to_strings_two_element_case() {
        let cols: Vec<ColumnName> = vec!["a".into(), "b".into()];
        let out = names_to_strings(&cols);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }
}
