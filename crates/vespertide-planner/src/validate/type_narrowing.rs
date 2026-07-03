//! Detect `ModifyColumnType` actions whose new type cannot losslessly hold
//! every value of the old type.
//!
//! This is fault **F6 / F19 / F33 / F87** in the data-dependent migration
//! fault taxonomy: shrinking a column's *type capacity* (VARCHAR length,
//! NUMERIC precision/scale, integer size, timestamp precision) lets a
//! database silently truncate, overflow, or reject existing rows. The
//! observable behaviour diverges per backend:
//!
//! | Backend      | `VARCHAR` shrink | `NUMERIC` shrink | Integer shrink |
//! |--------------|------------------|------------------|----------------|
//! | `PostgreSQL` | rejects (FAIL)   | rejects (FAIL)   | rejects (FAIL) |
//! | `MySQL`      | SILENT trim      | silent round     | silent overflow|
//! | `SQLite`     | ignored          | ignored          | type affinity  |
//!
//! Phase 1 (this module) is purely a **static detector**: it identifies
//! these transitions and surfaces them with backend-impact metadata so the
//! CLI can warn the user and the `revision` flow can force confirmation.
//! Phases 2 and 3 will add a `narrowing_strategy` field on the action so
//! Vespertide can emit *backend-uniform* pre-check / pre-update / pre-delete
//! SQL alongside the ALTER.

use vespertide_core::{
    ColumnType, ComplexColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableDef,
};

/// A single column whose `ModifyColumnType` action narrows the storable
/// value range. Returned by [`find_type_narrowings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeNarrowingWarning {
    /// Index of the offending action in the migration plan.
    pub action_index: usize,
    /// Table that owns the column.
    pub table: String,
    /// Column whose type is being narrowed.
    pub column: String,
    /// The kind of narrowing detected — also encodes the dimensions that shrank.
    pub kind: NarrowingKind,
    /// Human-readable rendering of the *old* type (from the baseline schema).
    /// e.g. `"varchar(40)"`, `"numeric(10,4)"`, `"bigint"`.
    pub from_display: String,
    /// Human-readable rendering of the *new* type (from the plan action).
    pub to_display: String,
}

/// Concrete shape of a single narrowing transition. Each variant encodes
/// exactly which dimension shrank so downstream code (CLI prompt, Phase 2
/// strategy applicability matrix, Phase 3 SQL generator) can dispatch on
/// the kind without re-parsing the types.
///
/// This enum is **not** `#[non_exhaustive]` because every future widening
/// of the detector should add a variant *here*, not silently fall through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NarrowingKind {
    /// `varchar(N1) -> varchar(N2)` with `N2 < N1`.
    VarcharLength { from: u32, to: u32 },
    /// `char(N1) -> char(N2)` with `N2 < N1`.
    CharLength { from: u32, to: u32 },
    /// `varchar(N) -> char(M)` with `M < N`. Treated as length narrowing.
    VarcharToCharShorter { from: u32, to: u32 },
    /// `char(N) -> varchar(M)` with `M < N`. Treated as length narrowing.
    CharToVarcharShorter { from: u32, to: u32 },
    /// `numeric(P1, S1) -> numeric(P2, S2)` where `S2 < S1`. Decimal places lost.
    NumericScale { from_scale: u32, to_scale: u32 },
    /// `numeric(P1, S1) -> numeric(P2, S2)` where `(P2 - S2) < (P1 - S1)`.
    /// Integer-part digits lost — overflow risk.
    NumericIntegerDigits {
        from_int_digits: u32,
        to_int_digits: u32,
    },
    /// Integer size shrinking, e.g. `bigint -> integer`, `integer -> smallint`.
    IntegerSize {
        from: &'static str,
        to: &'static str,
    },
    /// Float size shrinking, `double precision -> real`.
    FloatSize {
        from: &'static str,
        to: &'static str,
    },
    /// `text -> varchar(N)`. All rows >N chars will be affected.
    TextToVarchar { to_length: u32 },
    /// `text -> char(N)`. All rows >N chars will be affected.
    TextToChar { to_length: u32 },
    /// `timestamptz -> timestamp`. Timezone information lost — distinct from
    /// length/precision narrowing because the *semantic interpretation* of
    /// stored values shifts. Tracked here so the F20 timezone-prompt phase
    /// can hand off cleanly later.
    TimestamptzToTimestamp,
}

impl NarrowingKind {
    /// One-line description of `PostgreSQL`'s behaviour at `ALTER COLUMN` time.
    #[must_use]
    pub fn postgres_impact(&self) -> &'static str {
        match self {
            NarrowingKind::VarcharLength { .. }
            | NarrowingKind::CharLength { .. }
            | NarrowingKind::VarcharToCharShorter { .. }
            | NarrowingKind::CharToVarcharShorter { .. }
            | NarrowingKind::TextToVarchar { .. }
            | NarrowingKind::TextToChar { .. } => {
                "rejects ALTER with `value too long` if any row violates"
            }
            NarrowingKind::NumericScale { .. } | NarrowingKind::NumericIntegerDigits { .. } => {
                "rejects ALTER with `numeric field overflow` if any row violates"
            }
            NarrowingKind::IntegerSize { .. } => {
                "rejects ALTER with `out of range` if any row violates"
            }
            NarrowingKind::FloatSize { .. } => "silently loses precision (downcast)",
            NarrowingKind::TimestamptzToTimestamp => {
                "drops timezone; values reinterpreted as session timezone"
            }
        }
    }

    /// One-line description of `MySQL`'s behaviour at `ALTER COLUMN` time.
    #[must_use]
    pub fn mysql_impact(&self) -> &'static str {
        match self {
            NarrowingKind::VarcharLength { .. }
            | NarrowingKind::CharLength { .. }
            | NarrowingKind::VarcharToCharShorter { .. }
            | NarrowingKind::CharToVarcharShorter { .. }
            | NarrowingKind::TextToVarchar { .. }
            | NarrowingKind::TextToChar { .. } => {
                "SILENTLY truncates values past the new length (warning only)"
            }
            NarrowingKind::NumericScale { .. } => "silently rounds extra decimal digits",
            NarrowingKind::NumericIntegerDigits { .. } => {
                "rejects (or silently clips on non-strict sql_mode)"
            }
            NarrowingKind::IntegerSize { .. } => {
                "rejects on strict sql_mode; silently clamps otherwise"
            }
            NarrowingKind::FloatSize { .. } => "silently loses precision (downcast)",
            NarrowingKind::TimestamptzToTimestamp => "rebinds session timezone interpretation",
        }
    }

    /// One-line description of `SQLite`'s behaviour at `ALTER COLUMN` time.
    #[must_use]
    pub fn sqlite_impact(&self) -> &'static str {
        match self {
            NarrowingKind::VarcharLength { .. }
            | NarrowingKind::CharLength { .. }
            | NarrowingKind::VarcharToCharShorter { .. }
            | NarrowingKind::CharToVarcharShorter { .. }
            | NarrowingKind::TextToVarchar { .. }
            | NarrowingKind::TextToChar { .. } => "length advisory only — no enforcement",
            NarrowingKind::NumericScale { .. } | NarrowingKind::NumericIntegerDigits { .. } => {
                "stored as NUMERIC affinity; precision not enforced"
            }
            NarrowingKind::IntegerSize { .. } => "INTEGER affinity — no size enforcement",
            NarrowingKind::FloatSize { .. } => "REAL affinity — no size enforcement",
            NarrowingKind::TimestamptzToTimestamp => "stored as TEXT — no timezone semantics",
        }
    }
}

/// Scan a migration plan for `ModifyColumnType` actions that narrow a
/// column's storable value range. The baseline schema is required because
/// `ModifyColumnType` only carries the *new* type — the old type lives in
/// the schema reconstructed from applied migrations.
///
/// Static: this performs no data access; it only compares two `ColumnType`s
/// against a structural narrowing matrix.
#[must_use]
pub fn find_type_narrowings(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<TypeNarrowingWarning> {
    plan.actions
        .iter()
        .enumerate()
        .filter_map(|(idx, action)| warning_for_action(idx, action, baseline))
        .collect()
}

fn warning_for_action(
    idx: usize,
    action: &MigrationAction,
    baseline: &[TableDef],
) -> Option<TypeNarrowingWarning> {
    let MigrationAction::ModifyColumnType {
        table,
        column,
        new_type,
        ..
    } = action
    else {
        return None;
    };
    let old_type = baseline
        .iter()
        .find(|t| t.name == *table)?
        .columns
        .iter()
        .find(|c| c.name == *column)?
        .r#type
        .clone();
    let kind = is_narrowing(&old_type, new_type)?;
    Some(TypeNarrowingWarning {
        action_index: idx,
        table: table.to_string(),
        column: column.to_string(),
        kind,
        from_display: old_type.to_display_string(),
        to_display: new_type.to_display_string(),
    })
}

/// Pure type-comparison matrix. Returns `Some(kind)` iff `to` cannot
/// losslessly hold every value `from` can. Returns `None` for unchanged
/// types, widening transitions, and unrelated type swaps that this phase
/// deliberately does not classify.
#[must_use]
pub fn is_narrowing(from: &ColumnType, to: &ColumnType) -> Option<NarrowingKind> {
    use ColumnType::{Complex, Simple};
    use ComplexColumnType::{Char, Numeric, Varchar};
    use SimpleColumnType::{
        BigInt, DoublePrecision, Integer, Real, SmallInt, Text, Timestamp, Timestamptz,
    };

    match (from, to) {
        // --- VARCHAR / CHAR length narrowing -------------------------------
        (Complex(Varchar { length: a }), Complex(Varchar { length: b })) if b < a => {
            Some(NarrowingKind::VarcharLength { from: *a, to: *b })
        }
        (Complex(Char { length: a }), Complex(Char { length: b })) if b < a => {
            Some(NarrowingKind::CharLength { from: *a, to: *b })
        }
        (Complex(Varchar { length: a }), Complex(Char { length: b })) if b < a => {
            Some(NarrowingKind::VarcharToCharShorter { from: *a, to: *b })
        }
        (Complex(Char { length: a }), Complex(Varchar { length: b })) if b < a => {
            Some(NarrowingKind::CharToVarcharShorter { from: *a, to: *b })
        }
        // --- TEXT -> bounded length ---------------------------------------
        (Simple(Text), Complex(Varchar { length })) => {
            Some(NarrowingKind::TextToVarchar { to_length: *length })
        }
        (Simple(Text), Complex(Char { length })) => {
            Some(NarrowingKind::TextToChar { to_length: *length })
        }
        // --- NUMERIC precision/scale --------------------------------------
        (
            Complex(Numeric {
                precision: p1,
                scale: s1,
            }),
            Complex(Numeric {
                precision: p2,
                scale: s2,
            }),
        ) => {
            // Two independent dimensions: scale (decimal digits) and the
            // implicit integer-part width (precision - scale). Report only
            // whichever actually shrank. If both shrank we report scale,
            // because scale loss is the more commonly intended axis and a
            // single warning per action is cleaner; the impact descriptions
            // mention both possibilities anyway.
            if s2 < s1 {
                Some(NarrowingKind::NumericScale {
                    from_scale: *s1,
                    to_scale: *s2,
                })
            } else {
                let from_int = p1.saturating_sub(*s1);
                let to_int = p2.saturating_sub(*s2);
                if to_int < from_int {
                    Some(NarrowingKind::NumericIntegerDigits {
                        from_int_digits: from_int,
                        to_int_digits: to_int,
                    })
                } else {
                    None
                }
            }
        }
        // --- Integer size --------------------------------------------------
        (Simple(BigInt), Simple(Integer)) => Some(NarrowingKind::IntegerSize {
            from: "bigint",
            to: "integer",
        }),
        (Simple(BigInt), Simple(SmallInt)) => Some(NarrowingKind::IntegerSize {
            from: "bigint",
            to: "smallint",
        }),
        (Simple(Integer), Simple(SmallInt)) => Some(NarrowingKind::IntegerSize {
            from: "integer",
            to: "smallint",
        }),
        // --- Float size ----------------------------------------------------
        (Simple(DoublePrecision), Simple(Real)) => Some(NarrowingKind::FloatSize {
            from: "double precision",
            to: "real",
        }),
        // --- Timezone loss -------------------------------------------------
        (Simple(Timestamptz), Simple(Timestamp)) => Some(NarrowingKind::TimestamptzToTimestamp),
        // --- Anything else: widening, unchanged, or unrelated -------------
        _ => None,
    }
}
