use std::fmt;

use thiserror::Error;

/// Aggregates multiple [`PlannerError`]s into a single error so that batch
/// validators can report every violation at once.
///
/// The `Display` implementation renders a numbered list (1-indexed) of the
/// nested errors, preserving their order. Use this wherever multiple,
/// independently-meaningful failures must be surfaced from a single
/// validation pass — e.g. [`crate::validate::find_schema_violations`] or
/// [`crate::validate::find_plan_violations`].
#[derive(Debug)]
pub struct MultipleErrors(pub Vec<PlannerError>);

impl fmt::Display for MultipleErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} validation violation(s):", self.0.len())?;
        for (idx, err) in self.0.iter().enumerate() {
            writeln!(f, "  {}. {err}", idx + 1)?;
        }
        write!(f, "Fix all of the above before re-running this command.")
    }
}

impl std::error::Error for MultipleErrors {}

#[derive(Debug, Error)]
pub enum PlannerError {
    /// Wraps two or more independent [`PlannerError`]s reported in a single
    /// validation pass. Boxed via [`MultipleErrors`] to keep the enum size
    /// small (`Vec<PlannerError>` would otherwise inflate every variant).
    #[error("{0}")]
    Multiple(Box<MultipleErrors>),
    #[error("table already exists: {0}")]
    TableExists(String),
    #[error("table not found: {0}")]
    TableNotFound(String),
    #[error("column already exists: {0}.{1}")]
    ColumnExists(String, String),
    #[error("column not found: {0}.{1}")]
    ColumnNotFound(String, String),
    #[error("index not found: {0}.{1}")]
    IndexNotFound(String, String),
    #[error("duplicate table name: {0}")]
    DuplicateTableName(String),
    #[error("foreign key references non-existent table: {0}.{1} -> {2}")]
    ForeignKeyTableNotFound(String, String, String),
    #[error("foreign key references non-existent column: {0}.{1} -> {2}.{3}")]
    ForeignKeyColumnNotFound(String, String, String, String),
    #[error("index references non-existent column: {0}.{1} -> {2}")]
    IndexColumnNotFound(String, String, String),
    #[error("constraint references non-existent column: {0}.{1} -> {2}")]
    ConstraintColumnNotFound(String, String, String),
    #[error("constraint has empty column list: {0}.{1}")]
    EmptyConstraintColumns(String, String),
    #[error("AddColumn requires fill_with when column is NOT NULL without default: {0}.{1}")]
    MissingFillWith(String, String),
    /// A `data_migration` action whose SQL opens with a DDL keyword.
    ///
    /// `data_migration` is contractually schema-neutral: baseline replay skips
    /// it *because* it changes no schema. Hiding DDL inside one would drop that
    /// change from the reconstructed baseline permanently, after which
    /// `vespertide diff` reports the same already-applied changes on every run.
    /// Rejecting the plan at load time keeps the contract enforceable.
    #[error(
        "data_migration contains DDL: the statement starts with `{keyword}` ({statement}). \
         `data_migration` promises to change data only — baseline replay skips it on that \
         basis, so schema changes hidden here are lost forever and `vespertide diff` will \
         report phantom pending changes. Express the schema change with a typed action, or \
         use `raw_sql` if you genuinely need the escape hatch."
    )]
    DataMigrationContainsDdl {
        keyword: &'static str,
        statement: String,
    },
    #[error("table validation error: {0}")]
    TableValidation(String),
    #[error("table '{0}' must have a primary key")]
    MissingPrimaryKey(String),
    #[error("enum '{0}' in column '{1}.{2}' has duplicate variant name: '{3}'")]
    DuplicateEnumVariantName(String, String, String, String),
    #[error("enum '{0}' in column '{1}.{2}' has duplicate value: {3}")]
    DuplicateEnumValue(String, String, String, i64),
    #[error("{0}")]
    InvalidEnumDefault(#[from] Box<InvalidEnumDefaultError>),
    #[error(
        "auto_increment on non-integer column: {0}.{1} (type {2} does not support auto_increment)"
    )]
    InvalidAutoIncrement(String, String, String),
    #[error(
        "default value violates CHECK constraint: {table}.{column} default = {default_value} \
         fails CHECK ({check_expr}) — every INSERT relying on this default will be rejected by \
         the database. Change the default to satisfy the constraint, or relax the constraint."
    )]
    DefaultViolatesCheck {
        table: String,
        column: String,
        default_value: String,
        check_name: String,
        check_expr: String,
    },
    /// Fault **F-novel-15**: a CHECK constraint of the form
    /// `col BETWEEN low AND high` where `low > high` (literal boundaries
    /// swapped). SQL standard defines `BETWEEN` as
    /// `col >= low AND col <= high`; when `low > high` the conjunction
    /// is always false, so *every* `INSERT` is rejected by the
    /// database. Almost always an authoring error — the user intended
    /// `BETWEEN high AND low`. Reject the model up front rather than
    /// shipping a constraint that breaks every insert.
    #[error(
        "BETWEEN boundary order reversed: {table}.{column} CHECK '{check_name}' \
         is `BETWEEN {low} AND {high}` (low > high) — every row would be rejected by \
         the database. Swap the boundaries to `BETWEEN {high} AND {low}`."
    )]
    BetweenBoundaryReversed {
        table: String,
        column: String,
        check_name: String,
        low: String,
        high: String,
    },
    /// Fault **F-novel-1**: a CHECK constraint whose top-level
    /// conjuncts contain a *demonstrable* contradiction on the same
    /// column (e.g. `age > 100 AND age < 0`, `status = 'a' AND
    /// status = 'b'`, `col IS NULL AND col IS NOT NULL`). Every
    /// `INSERT` is rejected by the database because no value can
    /// satisfy both conjuncts simultaneously. Almost always an
    /// authoring error - the user transposed an operator, mixed up
    /// two columns, or copy-pasted a stale fragment.
    #[error(
        "CHECK self-contradiction: {table} CHECK '{check_name}' \
         contains conjuncts that cannot all be satisfied — \
         `{first}` and `{second}` reference column `{column}` but \
         demand disjoint values. Every row would be rejected by the \
         database. Reconcile or drop one of the conjuncts."
    )]
    CheckSelfContradiction {
        table: String,
        check_name: String,
        column: String,
        first: String,
        second: String,
    },
    /// Fault **F12 (Scenario C)**: a column declared with `nullable: true`
    /// participates in a `PRIMARY KEY`. SQL-92 defines `PRIMARY KEY` as
    /// `UNIQUE + NOT NULL`; `PostgreSQL`, `MySQL`, and `SQLite` (strict
    /// mode) all enforce this. Allowing the contradiction would either
    /// silently override `nullable` at SQL emit time or rely on `SQLite`'s
    /// historical bug behaviour for non-INTEGER-PK columns. Reject the
    /// model so the typed-schema promise stays portable.
    #[error(
        "primary key column nullable: {table}.{column} participates in a PRIMARY KEY \
         but declares `nullable: true`. SQL standard requires primary-key columns \
         to be NOT NULL. Either remove {column} from the primary key, or set \
         `nullable: false`. (For uniqueness with NULL allowed, use UNIQUE instead.)"
    )]
    PrimaryKeyColumnNullable { table: String, column: String },
    /// Fault **F12 (Scenario E)**: the plan removes a table's only PRIMARY
    /// KEY without adding a replacement (or dropping the table). Every
    /// Vespertide-managed table must have a primary key; without one
    /// the `SeaORM` exporter cannot produce a usable `Model` and replay
    /// against the baseline cannot match rows by identity.
    #[error(
        "table '{table}' would lose its PRIMARY KEY after this migration: \
         the plan removes PK on ({columns}) without adding a replacement, \
         and the table is not being dropped. Every Vespertide-managed table \
         must have a primary key — re-add one in the same migration, or drop \
         the table."
    )]
    PrimaryKeyRemovedWithoutReplacement { table: String, columns: String },
    /// Fault **F12 (Scenarios A/B)**: the plan swaps `PRIMARY KEY` and
    /// `UNIQUE` on the same column set within a single migration. Even
    /// though both constraints look similar, the swap silently changes
    /// every of the semantics tracked in the F12 design doc:
    ///
    /// - `PK → UQ` loses the implicit NOT NULL (existing rows may have
    ///   NULLs after a separate nullable change), changes FK semantics
    ///   (FK target was canonical, now optional), and turns single PK
    ///   into one of potentially many UNIQUE constraints.
    /// - `UQ → PK` adds implicit NOT NULL (the `ALTER` fails on every
    ///   backend if any row has NULL in those columns) and makes the
    ///   columns the canonical row identity (FK refs default to it).
    ///
    /// Vespertide blocks the swap so the user must explicitly express
    /// intent through a multi-migration sequence (or different column
    /// names). When foreign keys reference the column set, they are
    /// listed in `fk_references` so the user sees the downstream impact.
    ///
    /// `kind` carries the direction; `(table, columns)` identifies the
    /// affected constraint.
    #[error(
        "constraint type change blocked: {kind} on {table}.({columns}). \
         PRIMARY KEY and UNIQUE have different NOT NULL / FK / identity \
         semantics; vespertide refuses to swap them silently. \
         {fk_hint} \
         Split the change into separate migrations (e.g. add the new \
         constraint on a new column first, then drop the old one)."
    )]
    ConstraintTypeChanged {
        kind: &'static str,
        table: String,
        columns: String,
        /// Already-rendered hint like `"FKs referencing this column: \
        /// orders.user_id, posts.author_id."` or `""` when no FK targets it.
        fk_hint: String,
    },
    /// Fault **F3 Edge #1**: an `AddColumn` action that participates in a
    /// foreign key (inline `column.foreign_key` *or* a paired
    /// `AddConstraint(ForeignKey)` in the same plan) declares
    /// `nullable: false` while also carrying `fill_with` or a `default`.
    ///
    /// The F3 emit pipeline (1) inserts the column with the fill value,
    /// (2) NULL-ifies rows whose value does not exist in the referenced
    /// parent, then (3) adds the FK. Step (2) requires the column to be
    /// nullable. Vespertide refuses to silently lift `nullable` ? the
    /// user must declare it explicitly in the model.
    #[error(
        "AddColumn '{table}.{column}' participates in a foreign key but \
         declares `nullable: false` together with `fill_with`/`default`. \
         The migration emits the fill value first, then nullifies rows \
         whose value doesn't exist in the parent table ? this requires \
         `nullable: true`. Set `nullable: true` on {column}, or drop the \
         foreign key / fill_with."
    )]
    AddColumnWithFkRequiresNullable { table: String, column: String },

    /// Fault **F9**: a column or table is being dropped while a foreign key on
    /// another table still references it, with no matching cleanup in the
    /// same plan.
    ///
    /// Surviving foreign keys whose `(ref_table, ref_column)` is removed by
    /// this plan would silently break referential integrity once applied.
    /// The plan must either drop the offending FK (`RemoveConstraint`) or
    /// drop the entire referencing table in the same migration.
    ///
    /// - `dropped_table` is the table whose column or whole row is being removed.
    /// - `dropped_column` is `Some(col)` for `DeleteColumn`; `None` for
    ///   `DeleteTable` (the whole table is going away).
    /// - `referencing_table` is the *other* table whose FK is now dangling.
    /// - `referencing_constraint` is the FK constraint's declared name, or
    ///   `None` when the FK was inlined without an explicit name.
    #[error(
        "cannot drop {}: foreign key {} on table `{referencing_table}` references it. \
         Either drop that foreign key in the same migration, or drop the `{referencing_table}` table.",
        format_dropped(dropped_table, dropped_column.as_deref()),
        format_fk(referencing_constraint.as_deref()),
    )]
    DanglingForeignKeyAfterDrop {
        dropped_table: String,
        dropped_column: Option<String>,
        referencing_table: String,
        referencing_constraint: Option<String>,
    },
}

fn format_dropped(table: &str, column: Option<&str>) -> String {
    match column {
        Some(col) => format!("column `{table}.{col}`"),
        None => format!("table `{table}`"),
    }
}

fn format_fk(name: Option<&str>) -> String {
    match name {
        Some(n) => format!("`{n}`"),
        None => "(unnamed)".to_string(),
    }
}

/// An enum column has a default or `fill_with` value not in the allowed set.
#[derive(Debug, Error)]
#[error(
    "enum '{enum_name}' in column '{table_name}.{column_name}' has invalid {value_type} value '{value}': not in allowed values [{allowed}]"
)]
pub struct InvalidEnumDefaultError {
    pub enum_name: String,
    pub table_name: String,
    pub column_name: String,
    pub value_type: String,
    pub value: String,
    pub allowed: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Coverage-closure: `DanglingForeignKeyAfterDrop` Display formatting
    /// across all four `(dropped_column, referencing_constraint)` quadrants.
    /// Exercises both arms of `format_dropped` (column vs whole-table) and
    /// `format_fk` (named vs unnamed FK) so every doc-line / format-string
    /// slice inside the `#[error(...)]` attribute is reached.
    #[test]
    fn dangling_fk_after_drop_column_drop_named_fk_displays_full_message() {
        let err = PlannerError::DanglingForeignKeyAfterDrop {
            dropped_table: "user".to_string(),
            dropped_column: Some("id".to_string()),
            referencing_table: "post".to_string(),
            referencing_constraint: Some("fk_post_user".to_string()),
        };
        let msg = err.to_string();
        assert!(msg.contains("column `user.id`"), "msg: {msg}");
        assert!(msg.contains("`fk_post_user`"), "msg: {msg}");
        assert!(msg.contains("on table `post`"), "msg: {msg}");
    }

    #[test]
    fn dangling_fk_after_drop_table_drop_unnamed_fk_displays_unnamed_marker() {
        let err = PlannerError::DanglingForeignKeyAfterDrop {
            dropped_table: "parent".to_string(),
            dropped_column: None,
            referencing_table: "child".to_string(),
            referencing_constraint: None,
        };
        let msg = err.to_string();
        // format_dropped(None) → "table `parent`"; format_fk(None) → "(unnamed)"
        assert!(msg.contains("table `parent`"), "msg: {msg}");
        assert!(msg.contains("(unnamed)"), "msg: {msg}");
        assert!(msg.contains("on table `child`"), "msg: {msg}");
    }

    #[test]
    fn dangling_fk_after_drop_column_drop_unnamed_fk_combines_both_arms() {
        let err = PlannerError::DanglingForeignKeyAfterDrop {
            dropped_table: "user".to_string(),
            dropped_column: Some("email".to_string()),
            referencing_table: "log".to_string(),
            referencing_constraint: None,
        };
        let msg = err.to_string();
        assert!(msg.contains("column `user.email`"), "msg: {msg}");
        assert!(msg.contains("(unnamed)"), "msg: {msg}");
    }

    #[test]
    fn dangling_fk_after_drop_table_drop_named_fk_combines_both_arms() {
        let err = PlannerError::DanglingForeignKeyAfterDrop {
            dropped_table: "parent".to_string(),
            dropped_column: None,
            referencing_table: "child".to_string(),
            referencing_constraint: Some("fk_child_parent".to_string()),
        };
        let msg = err.to_string();
        assert!(msg.contains("table `parent`"), "msg: {msg}");
        assert!(msg.contains("`fk_child_parent`"), "msg: {msg}");
    }

    /// Verify the helper free functions directly so both match arms are
    /// reached even if the macro-expanded Display path skips one.
    #[test]
    fn format_dropped_helpers_both_arms() {
        assert_eq!(format_dropped("user", Some("id")), "column `user.id`");
        assert_eq!(format_dropped("user", None), "table `user`");
    }

    #[test]
    fn format_fk_helpers_both_arms() {
        assert_eq!(format_fk(Some("fk_a")), "`fk_a`");
        assert_eq!(format_fk(None), "(unnamed)");
    }

    /// Coverage-closure: `MultipleErrors` Display path with several
    /// nested errors. Ensures the `for (idx, err)` numbered-list arm
    /// in `MultipleErrors::fmt` is reached.
    #[test]
    fn multiple_errors_renders_numbered_list() {
        let multi = MultipleErrors(vec![
            PlannerError::TableExists("user".to_string()),
            PlannerError::ColumnNotFound("user".to_string(), "email".to_string()),
        ]);
        let s = multi.to_string();
        assert!(s.contains("2 validation violation(s):"), "{s}");
        assert!(s.contains("1. table already exists: user"), "{s}");
        assert!(s.contains("2. column not found: user.email"), "{s}");
        assert!(s.contains("Fix all of the above"), "{s}");
    }
}
