use super::*;
use rstest::rstest;
use vespertide_core::SimpleColumnType;
use vespertide_planner::{
    CascadeRiskLevel, CheckStrengtheningKind, DefaultChangeKind, PkKind, SequenceExhaustionKind,
    SequenceRiskLevel, UniqueAdditionFkReference as FkReference,
};

// Strip ANSI escape sequences so substring assertions are robust to the
// `colored` crate's TTY-detection (it might or might not colorize under
// `cargo test`).
fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c.is_ascii_alphabetic() {
                in_esc = false;
            }
            continue;
        }
        if c == '\u{1b}' {
            in_esc = true;
            continue;
        }
        out.push(c);
    }
    out
}

// ── format_default_change_header ─────────────────────────────────────

fn default_warning(
    kind: DefaultChangeKind,
    old: Option<&str>,
    new: Option<&str>,
) -> DefaultChangeWarning {
    DefaultChangeWarning {
        action_index: 0,
        table: "users".into(),
        column: "status".into(),
        old_default: old.map(str::to_string),
        new_default: new.map(str::to_string),
        kind,
    }
}

#[rstest]
#[case::high(DefaultChangeKind::LiteralToFunction, "HIGH RISK")]
#[case::medium_added(DefaultChangeKind::AddedDefault, "MEDIUM RISK")]
#[case::medium_removed(DefaultChangeKind::RemovedDefault, "MEDIUM RISK")]
#[case::medium_fn_to_lit(DefaultChangeKind::FunctionToLiteral, "MEDIUM RISK")]
#[case::low_lit_to_lit(DefaultChangeKind::LiteralToLiteral, "LOW RISK")]
#[case::low_fn_to_fn(DefaultChangeKind::FunctionToFunction, "LOW RISK")]
fn format_default_change_header_emits_risk_label(
    #[case] kind: DefaultChangeKind,
    #[case] expected_label: &str,
) {
    let w = default_warning(kind, Some("'a'"), Some("'b'"));
    let header = strip(&format_default_change_header(&w));
    assert!(
        header.contains(expected_label),
        "expected risk label `{expected_label}` in: {header}"
    );
    assert!(header.contains("Column DEFAULT change"));
    assert!(header.contains("users"));
    assert!(header.contains("status"));
    assert!(header.contains("Existing rows are NOT automatically updated."));
}

#[test]
fn format_default_change_header_renders_none_as_placeholder() {
    let w = default_warning(DefaultChangeKind::AddedDefault, None, Some("'a'"));
    let header = strip(&format_default_change_header(&w));
    assert!(
        header.contains("(none)"),
        "missing `(none)` for old=None: {header}"
    );
    let w2 = default_warning(DefaultChangeKind::RemovedDefault, Some("'a'"), None);
    let header2 = strip(&format_default_change_header(&w2));
    assert!(
        header2.contains("(none)"),
        "missing `(none)` for new=None: {header2}"
    );
}

// ── format_unique_addition_header ────────────────────────────────────

fn uniq_warning(pk_kind: PkKind, fk_refs: Vec<FkReference>) -> UniqueAdditionWarning {
    UniqueAdditionWarning {
        action_index: 0,
        table: "users".into(),
        constraint_name: Some("uq".into()),
        columns: vec!["email".into(), "tenant_id".into()],
        pk_kind,
        fk_references: fk_refs,
    }
}

#[test]
fn format_unique_addition_header_single_auto_cleanup_renders_pk_hint() {
    let w = uniq_warning(
        PkKind::SingleAutoCleanupCapable {
            column: "id".into(),
        },
        vec![],
    );
    let h = strip(&format_unique_addition_header(&w));
    assert!(h.contains("Adding UNIQUE on"));
    assert!(h.contains("users.(email, tenant_id)"));
    assert!(h.contains("Single-column PK: id"));
    assert!(h.contains("auto-cleanup available"));
    assert!(!h.contains("Foreign keys reference"));
}

#[test]
fn format_unique_addition_header_inside_unique_set_renders_tautology_hint() {
    let w = uniq_warning(
        PkKind::SingleInsideUniqueSet {
            column: "email".into(),
        },
        vec![],
    );
    let h = strip(&format_unique_addition_header(&w));
    assert!(h.contains("INSIDE the unique set"));
    assert!(h.contains("tautology"));
}

#[test]
fn format_unique_addition_header_composite_pk_renders_columns() {
    let w = uniq_warning(
        PkKind::Composite {
            columns: vec!["a".into(), "b".into()],
        },
        vec![],
    );
    let h = strip(&format_unique_addition_header(&w));
    assert!(h.contains("Composite PK (a, b)"));
    assert!(h.contains("Pre-clean manually"));
}

#[test]
fn format_unique_addition_header_no_pk_renders_defensive_hint() {
    let w = uniq_warning(PkKind::None, vec![]);
    let h = strip(&format_unique_addition_header(&w));
    assert!(h.contains("No PRIMARY KEY"));
}

#[test]
fn format_unique_addition_header_fk_refs_with_and_without_constraint_name() {
    let fks = vec![
        FkReference {
            child_table: "posts".into(),
            constraint_name: Some("fk_p".into()),
            child_columns: vec!["user_id".into()],
        },
        FkReference {
            child_table: "audit".into(),
            constraint_name: None,
            child_columns: vec!["uid".into(), "tid".into()],
        },
    ];
    let w = uniq_warning(
        PkKind::SingleAutoCleanupCapable {
            column: "id".into(),
        },
        fks,
    );
    let h = strip(&format_unique_addition_header(&w));
    assert!(h.contains("Foreign keys reference"));
    assert!(h.contains("posts.fk_p"));
    // Unnamed FK falls back to "(child_columns)"
    assert!(h.contains("audit.(uid, tid)"));
}

// ── format_fk_orphan_addition_header ─────────────────────────────────

fn fk_orphan_warning_h(nullable: bool, constraint_name: Option<&str>) -> FkOrphanAdditionWarning {
    FkOrphanAdditionWarning {
        action_index: 0,
        table: "post".into(),
        constraint_name: constraint_name.map(str::to_string),
        columns: vec!["user_id".into()],
        ref_table: "user".into(),
        ref_columns: vec!["id".into()],
        all_columns_nullable: nullable,
    }
}

#[rstest]
#[case::nullable_named(
    true,
    Some("fk_post_user"),
    "Nullify is available",
    " (constraint `fk_post_user`)"
)]
#[case::not_null_unnamed(false, None, "only Delete is available", "Adding FOREIGN KEY")]
fn format_fk_orphan_addition_header_branches(
    #[case] nullable: bool,
    #[case] name: Option<&str>,
    #[case] expected_hint: &str,
    #[case] expected_label_fragment: &str,
) {
    let w = fk_orphan_warning_h(nullable, name);
    let h = strip(&format_fk_orphan_addition_header(&w));
    assert!(
        h.contains(expected_hint),
        "missing hint `{expected_hint}` in: {h}"
    );
    assert!(
        h.contains(expected_label_fragment),
        "missing label fragment `{expected_label_fragment}` in: {h}"
    );
    assert!(h.contains("post.(user_id)"));
    assert!(h.contains("user.(id)"));
}

// ── format_check_addition_header ─────────────────────────────────────

fn check_addition_warning_h(nullable: bool) -> CheckAdditionWarning {
    CheckAdditionWarning {
        action_index: 0,
        table: "products".into(),
        constraint_name: "chk_price".into(),
        check_expr: "price > 0".into(),
        target_column: "price".into(),
        target_column_nullable: nullable,
    }
}

#[rstest]
#[case::nullable(true, "Nullify is available")]
#[case::not_null(false, "only Delete is available")]
fn format_check_addition_header_branches(#[case] nullable: bool, #[case] expected: &str) {
    let h = strip(&format_check_addition_header(&check_addition_warning_h(
        nullable,
    )));
    assert!(h.contains("Adding CHECK"));
    assert!(h.contains("chk_price"));
    assert!(h.contains("price > 0"));
    assert!(h.contains("products.price"));
    assert!(h.contains(expected));
}

// ── format_pk_addition_header ────────────────────────────────────────

fn pk_warning_h(
    nullable_cols: Vec<&str>,
    duplicate_possible: bool,
    auto_cleanup_capable: bool,
) -> PrimaryKeyAdditionWarning {
    PrimaryKeyAdditionWarning {
        action_index: 0,
        table: "users".into(),
        columns: vec!["id".into()],
        kind: vespertide_planner::PkAdditionKind::ExistingColumns,
        nullable_columns: nullable_cols.into_iter().map(str::to_string).collect(),
        duplicate_possible,
        auto_cleanup_capable,
    }
}

#[test]
fn format_pk_addition_header_dedup_auto_cleanup_available() {
    let h = strip(&format_pk_addition_header(&pk_warning_h(
        vec![],
        true,
        true,
    )));
    assert!(h.contains("Adding PRIMARY KEY"));
    assert!(h.contains("users.(id)"));
    assert!(h.contains("Auto-cleanup available"));
    assert!(!h.contains("Nullable PK columns"));
}

#[test]
fn format_pk_addition_header_dedup_composite_no_auto_cleanup() {
    let h = strip(&format_pk_addition_header(&pk_warning_h(
        vec![],
        true,
        false,
    )));
    assert!(h.contains("Composite PK") || h.contains("no baseline PK"));
}

#[test]
fn format_pk_addition_header_dedup_baseline_unique_prevents() {
    let h = strip(&format_pk_addition_header(&pk_warning_h(
        vec![],
        false,
        false,
    )));
    assert!(h.contains("Baseline UNIQUE already prevents duplicates"));
}

#[test]
fn format_pk_addition_header_nullable_columns_listed() {
    let h = strip(&format_pk_addition_header(&pk_warning_h(
        vec!["a", "b"],
        true,
        true,
    )));
    assert!(h.contains("Nullable PK columns: a, b"));
    assert!(h.contains("fill_with prompt"));
}

// ── warning_is_mutable ───────────────────────────────────────────────

#[rstest]
#[case::primary(SequenceExhaustionKind::Primary, true)]
#[case::pk_type_narrowing(SequenceExhaustionKind::PkTypeNarrowing { from: SimpleColumnType::BigInt }, true)]
#[case::fk_mismatch(SequenceExhaustionKind::ForeignKeyMismatch { parent_table: "p".into(), parent_type: SimpleColumnType::BigInt }, false)]
fn warning_is_mutable_matches_kind(#[case] kind: SequenceExhaustionKind, #[case] expected: bool) {
    let w = SequenceExhaustionWarning {
        action_index: 0,
        table: "t".into(),
        column: "c".into(),
        current_type: SimpleColumnType::Integer,
        recommended_type: SimpleColumnType::BigInt,
        risk_level: SequenceRiskLevel::Medium,
        kind,
    };
    assert_eq!(warning_is_mutable(&w), expected);
}

// ── simple_int_label ─────────────────────────────────────────────────

#[rstest]
#[case::small_int(SimpleColumnType::SmallInt, "small_int")]
#[case::integer(SimpleColumnType::Integer, "integer")]
#[case::big_int(SimpleColumnType::BigInt, "big_int")]
#[case::other(SimpleColumnType::Text, "?")]
fn simple_int_label_returns_expected_string(
    #[case] ty: SimpleColumnType,
    #[case] expected: &'static str,
) {
    assert_eq!(simple_int_label(ty), expected);
}

// ── format_sequence_exhaustion_header ────────────────────────────────

fn seq_warning_h(
    kind: SequenceExhaustionKind,
    current: SimpleColumnType,
    risk: SequenceRiskLevel,
) -> SequenceExhaustionWarning {
    SequenceExhaustionWarning {
        action_index: 0,
        table: "events".into(),
        column: "id".into(),
        current_type: current,
        recommended_type: SimpleColumnType::BigInt,
        risk_level: risk,
        kind,
    }
}

#[test]
fn format_sequence_exhaustion_header_primary_kind_integer_risk_medium() {
    let h = strip(&format_sequence_exhaustion_header(&seq_warning_h(
        SequenceExhaustionKind::Primary,
        SimpleColumnType::Integer,
        SequenceRiskLevel::Medium,
    )));
    assert!(h.contains("INT identity overflow risk"));
    assert!(h.contains("Target: events.id (integer)"));
    assert!(h.contains("single-column auto-increment PRIMARY KEY"));
    assert!(h.contains("Risk: Medium"));
    assert!(h.contains("1M new rows/day"));
    assert!(h.contains("Recommended: rewrite to big_int"));
}

#[test]
fn format_sequence_exhaustion_header_primary_kind_smallint_risk_high() {
    let h = strip(&format_sequence_exhaustion_header(&seq_warning_h(
        SequenceExhaustionKind::Primary,
        SimpleColumnType::SmallInt,
        SequenceRiskLevel::High,
    )));
    assert!(h.contains("Risk: High"));
    assert!(h.contains("hours to days"));
    assert!(h.contains("(small_int)"));
}

#[test]
fn format_sequence_exhaustion_header_pk_narrowing_scenario() {
    let h = strip(&format_sequence_exhaustion_header(&seq_warning_h(
        SequenceExhaustionKind::PkTypeNarrowing {
            from: SimpleColumnType::BigInt,
        },
        SimpleColumnType::Integer,
        SequenceRiskLevel::Medium,
    )));
    assert!(h.contains("PRIMARY KEY type narrowing from big_int to integer"));
}

#[test]
fn format_sequence_exhaustion_header_fk_mismatch_scenario() {
    let h = strip(&format_sequence_exhaustion_header(&seq_warning_h(
        SequenceExhaustionKind::ForeignKeyMismatch {
            parent_table: "users".into(),
            parent_type: SimpleColumnType::BigInt,
        },
        SimpleColumnType::Integer,
        SequenceRiskLevel::Medium,
    )));
    assert!(h.contains("FOREIGN KEY mismatch"));
    assert!(h.contains("users"));
    assert!(h.contains("big_int"));
}

#[test]
fn format_sequence_exhaustion_header_other_current_type_omits_estimate() {
    // current_type outside SmallInt/Integer => no estimate line (the `_ =>
    // String::new()` arm in the estimate match).
    let h = strip(&format_sequence_exhaustion_header(&seq_warning_h(
        SequenceExhaustionKind::Primary,
        SimpleColumnType::BigInt,
        SequenceRiskLevel::Medium,
    )));
    assert!(!h.contains("rows/day"));
    assert!(!h.contains("hours to days"));
}

// ── format_cascade_reach_header ──────────────────────────────────────

fn cascade_warning_h(
    risk: CascadeRiskLevel,
    reached: Vec<&str>,
    depth: usize,
    max_fanout: usize,
) -> CascadeReachWarning {
    CascadeReachWarning {
        action_index: 0,
        origin_child_table: "posts".into(),
        origin_columns: vec!["user_id".into()],
        parent_table: "users".into(),
        depth,
        reached_tables: reached.into_iter().map(str::to_string).collect(),
        max_fanout,
        risk_level: risk,
    }
}

#[rstest]
#[case::deep(CascadeRiskLevel::Deep, "Deep")]
#[case::high_fanout(CascadeRiskLevel::HighFanout, "HighFanout")]
#[case::critical(CascadeRiskLevel::Critical, "Critical")]
fn format_cascade_reach_header_risk_label(#[case] risk: CascadeRiskLevel, #[case] expected: &str) {
    let h = strip(&format_cascade_reach_header(&cascade_warning_h(
        risk,
        vec!["comments", "tags"],
        3,
        4,
    )));
    assert!(h.contains("ON DELETE CASCADE chain warning"));
    assert!(h.contains("posts.(user_id)"));
    assert!(h.contains("Cascade reach: 3 hops"));
    assert!(h.contains("users \u{2192} comments \u{2192} tags"));
    assert!(h.contains(&format!("Risk: {expected}")));
    assert!(h.contains("depth=3, max fanout=4"));
}

// ── format_check_strengthening_header ────────────────────────────────

fn check_strengthening_warning_h(kind: CheckStrengtheningKind) -> CheckStrengtheningWarning {
    CheckStrengtheningWarning {
        action_index: 0,
        table: "products".into(),
        constraint_name: "chk".into(),
        old_expr: "price > 0".into(),
        new_expr: "price > 10".into(),
        kind,
    }
}

#[rstest]
#[case::boundary(CheckStrengtheningKind::BoundaryTightened, "boundary tightened")]
#[case::operator(CheckStrengtheningKind::OperatorTightened, "operator tightened")]
#[case::in_list(CheckStrengtheningKind::InListShrunk, "IN list shrunk")]
#[case::between(CheckStrengtheningKind::BetweenNarrowed, "BETWEEN range narrowed")]
#[case::conjunct(CheckStrengtheningKind::ConjunctAdded, "extra AND conjunct added")]
#[case::disjunct(CheckStrengtheningKind::DisjunctRemoved, "OR disjunct removed")]
fn format_check_strengthening_header_kind_label(
    #[case] kind: CheckStrengtheningKind,
    #[case] expected: &str,
) {
    let h = strip(&format_check_strengthening_header(
        &check_strengthening_warning_h(kind),
    ));
    assert!(h.contains("CHECK expression strengthened"));
    assert!(h.contains("products"));
    assert!(h.contains("chk"));
    assert!(h.contains("price > 0"));
    assert!(h.contains("price > 10"));
    assert!(
        h.contains(expected),
        "missing kind label `{expected}` in: {h}"
    );
}

// ── format_check_type_mismatch_header ────────────────────────────────

#[test]
fn format_check_type_mismatch_header_renders_all_fields() {
    let w = CheckTypeMismatchWarning {
        action_index: 0,
        table: "orders".into(),
        constraint_name: "chk_qty".into(),
        column: "qty".into(),
        column_type_label: "integer".into(),
        literal_text: "'abc'".into(),
        literal_kind: "String".into(),
        expr: "qty = 'abc'".into(),
    };
    let h = strip(&format_check_type_mismatch_header(&w));
    assert!(h.contains("CHECK literal type mismatch"));
    assert!(h.contains("orders"));
    assert!(h.contains("chk_qty"));
    assert!(h.contains("qty (integer)"));
    assert!(h.contains("'abc' (String)"));
    assert!(h.contains("qty = 'abc'"));
    assert!(h.contains("PostgreSQL rejects this at ADD CONSTRAINT time"));
}
