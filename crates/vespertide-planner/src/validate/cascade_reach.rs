//! Fault **F96** - ON DELETE CASCADE chain reach analysis.
//!
//! Adding a new `FOREIGN KEY ... ON DELETE CASCADE` extends the
//! database's automatic-deletion graph. A seemingly-narrow `DELETE
//! FROM parent WHERE id = 42;` can then silently cascade to thousands
//! of rows across many downstream tables - a production-incident
//! pattern where operators discover the data loss hours or days
//! later via dashboards rather than at delete time.
//!
//! Vespertide statically analyses **every newly added** `ON DELETE
//! CASCADE` foreign key against the baseline FK graph and surfaces a
//! warning when:
//!
//! - **Deep** (chain depth >= 3) - the cascade reaches at least three
//!   downstream tables.
//! - **`HighFanout`** (any node has >= 3 cascade children) - a single
//!   parent fanouts into many downstream tables.
//! - **Critical** - both conditions hold simultaneously.
//!
//! Shallow / narrow chains (depth < 3 and max fanout < 3) are not
//! reported - they are the normal one-to-one parent-child pattern.
//!
//! ## Scope notes
//!
//! - Only `ON DELETE CASCADE` is analysed. `ON UPDATE CASCADE` is a
//!   row-update concern (not a delete-reach concern) and is out of
//!   scope.
//! - Baseline FKs that *already* form a deep chain (i.e. independent
//!   of this plan) are **not reported**. The user has already lived
//!   with those - F96 surfaces only the additional reach introduced
//!   by *this* migration.
//! - Self-referential FKs (e.g. `categories.parent_id ->
//!   categories.id`) are cycle-detected via the visited set during
//!   DFS; tree-shaped schemas don't false-positive.
//! - `RESTRICT` / `SET NULL` / `SET DEFAULT` / `NO ACTION` foreign
//!   keys do not propagate deletes and are excluded from the graph.
//! - The cascade graph is built from `baseline` plus this plan's
//!   `AddConstraint(ForeignKey)` additions. Removals via
//!   `RemoveConstraint` are not subtracted (the user removing a
//!   CASCADE FK would shrink the chain - never grow it).

use std::collections::{BTreeMap, HashSet};

use vespertide_core::{MigrationAction, MigrationPlan, ReferenceAction, TableConstraint, TableDef};

/// Depth at or above which a cascade chain is flagged as "Deep".
const DEEP_THRESHOLD: usize = 3;
/// Per-node cascade-child count at or above which the chain is
/// flagged as `HighFanout`.
const HIGH_FANOUT_THRESHOLD: usize = 3;

/// One risky `ON DELETE CASCADE` addition surfaced for user
/// confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadeReachWarning {
    /// Index of the `AddConstraint(ForeignKey)` action in the plan.
    pub action_index: usize,
    /// Child table that owns the newly added FK (`posts` in
    /// `posts.user_id -> users.id ON DELETE CASCADE`).
    pub origin_child_table: String,
    /// Child FK columns.
    pub origin_columns: Vec<String>,
    /// Parent (referenced) table - the root of the cascade subgraph
    /// rooted at the new FK.
    pub parent_table: String,
    /// Maximum hop-count from `parent_table` to any reachable
    /// descendant, counted across only `ON DELETE CASCADE` edges. A
    /// depth of 0 means no cascade descendants; 1 means a single
    /// direct child; 3+ triggers the `Deep` label.
    pub depth: usize,
    /// Distinct tables reachable from `parent_table` via cascade
    /// edges, in DFS visitation order. Excludes the parent itself.
    pub reached_tables: Vec<String>,
    /// Maximum cascade-child count observed at any node on the chain.
    pub max_fanout: usize,
    /// Classifier label combining depth and fanout.
    pub risk_level: CascadeRiskLevel,
}

/// Classifier label for a cascade chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadeRiskLevel {
    /// Chain depth >= `DEEP_THRESHOLD`, fanout below threshold.
    Deep,
    /// Some node has fanout >= `HIGH_FANOUT_THRESHOLD`, depth below
    /// `DEEP_THRESHOLD`.
    HighFanout,
    /// Both `Deep` and `HighFanout` conditions hold.
    Critical,
}

/// Scan the plan for newly added `ON DELETE CASCADE` foreign keys
/// and emit a warning when the resulting cascade chain (computed
/// against the union of `baseline` plus this plan's CASCADE-FK
/// additions) reaches the depth/fanout thresholds.
///
/// Returns warnings in plan-order. Empty when every newly added
/// CASCADE FK extends only a shallow, narrow chain.
#[must_use]
pub fn find_cascade_reach_violations(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<CascadeReachWarning> {
    let graph = build_cascade_graph(baseline, plan);

    let mut warnings = Vec::new();
    for (idx, action) in plan.actions.iter().enumerate() {
        let MigrationAction::AddConstraint {
            table,
            constraint:
                TableConstraint::ForeignKey {
                    columns,
                    ref_table,
                    on_delete: Some(ReferenceAction::Cascade),
                    ..
                },
        } = action
        else {
            continue;
        };

        let (depth, reached, max_fanout) = dfs_cascade(&graph, ref_table.as_str());
        let Some(risk_level) = classify_risk(depth, max_fanout) else {
            continue;
        };

        warnings.push(CascadeReachWarning {
            action_index: idx,
            origin_child_table: table.to_string(),
            origin_columns: columns.iter().map(ToString::to_string).collect(),
            parent_table: ref_table.to_string(),
            depth,
            reached_tables: reached,
            max_fanout,
            risk_level,
        });
    }
    warnings
}

/// Build the cascade-edge graph used by [`find_cascade_reach_violations`].
///
/// An edge `parent -> child` exists whenever `child` declares a
/// `FOREIGN KEY ... REFERENCES parent ON DELETE CASCADE`. The graph
/// is the **union** of baseline FKs and this plan's additions; plan
/// removals are not subtracted (a CASCADE removal can only shrink
/// the chain, never grow it).
fn build_cascade_graph(
    baseline: &[TableDef],
    plan: &MigrationPlan,
) -> BTreeMap<String, Vec<String>> {
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for table in baseline {
        for constraint in &table.constraints {
            if let TableConstraint::ForeignKey {
                ref_table,
                on_delete: Some(ReferenceAction::Cascade),
                ..
            } = constraint
            {
                graph
                    .entry(ref_table.to_string())
                    .or_default()
                    .push(table.name.to_string());
            }
        }
    }

    for action in &plan.actions {
        if let MigrationAction::AddConstraint {
            table,
            constraint:
                TableConstraint::ForeignKey {
                    ref_table,
                    on_delete: Some(ReferenceAction::Cascade),
                    ..
                },
        } = action
        {
            graph
                .entry(ref_table.to_string())
                .or_default()
                .push(table.to_string());
        }
    }

    graph
}

/// Iterative DFS that measures the cascade subgraph rooted at
/// `start`. Returns `(depth, reached_tables, max_fanout)` where
/// `depth` is the maximum hop-count from `start` to any reachable
/// node and `max_fanout` is the largest single-node out-degree
/// observed along the walk. `start` itself is excluded from
/// `reached_tables`.
fn dfs_cascade(graph: &BTreeMap<String, Vec<String>>, start: &str) -> (usize, Vec<String>, usize) {
    let mut visited: HashSet<String> = HashSet::new();
    let mut max_depth = 0;
    let mut max_fanout = 0;
    let mut reached: Vec<String> = Vec::new();

    let mut stack: Vec<(String, usize)> = vec![(start.to_string(), 0)];
    while let Some((node, depth)) = stack.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if depth > 0 {
            reached.push(node.clone());
            max_depth = max_depth.max(depth);
        }
        if let Some(children) = graph.get(&node) {
            max_fanout = max_fanout.max(children.len());
            for child in children {
                stack.push((child.clone(), depth + 1));
            }
        }
    }

    (max_depth, reached, max_fanout)
}

/// Classify a `(depth, max_fanout)` pair into a `CascadeRiskLevel`,
/// or `None` when neither dimension reaches the threshold. The
/// thresholds (`DEEP_THRESHOLD`, `HIGH_FANOUT_THRESHOLD`) are tuned
/// to skip normal one-to-one parent-child patterns and surface only
/// genuinely unusual cascade shapes.
fn classify_risk(depth: usize, max_fanout: usize) -> Option<CascadeRiskLevel> {
    let deep = depth >= DEEP_THRESHOLD;
    let high_fanout = max_fanout >= HIGH_FANOUT_THRESHOLD;
    match (deep, high_fanout) {
        (true, true) => Some(CascadeRiskLevel::Critical),
        (true, false) => Some(CascadeRiskLevel::Deep),
        (false, true) => Some(CascadeRiskLevel::HighFanout),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, ForeignKeyOrphanStrategy, MigrationAction, MigrationPlan,
        ReferenceAction, SimpleColumnType, TableConstraint, TableDef, TableName,
    };

    fn col(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    /// Create a `TableDef` whose declared FKs *all* use ON DELETE
    /// CASCADE - convenient shorthand for building cascade-chain
    /// fixtures.
    fn table_with_cascade_fks(
        name: &str,
        cols: Vec<&str>,
        cascade_fks: Vec<(&str, &str)>, // (fk_col, parent_table)
    ) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: cols.into_iter().map(col).collect(),
            constraints: cascade_fks
                .into_iter()
                .map(|(fk_col, parent)| TableConstraint::ForeignKey {
                    name: None,
                    columns: vec![fk_col.into()],
                    ref_table: parent.into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: ForeignKeyOrphanStrategy::default(),
                })
                .collect(),
        }
    }

    fn add_cascade_fk(table: &str, col: &str, parent: &str) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: TableName::from(table),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec![col.into()],
                ref_table: parent.into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            },
        }
    }

    fn add_restrict_fk(table: &str, col: &str, parent: &str) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: TableName::from(table),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec![col.into()],
                ref_table: parent.into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Restrict),
                on_update: None,
                orphan_strategy: ForeignKeyOrphanStrategy::default(),
            },
        }
    }

    fn plan(actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: "test".into(),
            version: 1,
            comment: None,
            created_at: None,
            actions,
        }
    }

    #[rstest]
    fn case_01_shallow_narrow_chain_no_warning() {
        // posts -> users only (depth 1) — normal pattern.
        let baseline = vec![
            table_with_cascade_fks("users", vec!["id"], vec![]),
            table_with_cascade_fks("posts", vec!["id", "user_id"], vec![]),
        ];
        let p = plan(vec![add_cascade_fk("posts", "user_id", "users")]);
        assert!(find_cascade_reach_violations(&p, &baseline).is_empty());
    }

    #[rstest]
    fn case_02_deep_chain_three_hops_flagged_deep() {
        // users <- posts <- comments <- reactions (depth 3 from
        // adding posts -> users)
        let baseline = vec![
            table_with_cascade_fks("users", vec!["id"], vec![]),
            table_with_cascade_fks("posts", vec!["id", "user_id"], vec![]),
            table_with_cascade_fks(
                "comments",
                vec!["id", "post_id"],
                vec![("post_id", "posts")],
            ),
            table_with_cascade_fks(
                "reactions",
                vec!["id", "comment_id"],
                vec![("comment_id", "comments")],
            ),
        ];
        let p = plan(vec![add_cascade_fk("posts", "user_id", "users")]);
        let ws = find_cascade_reach_violations(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].risk_level, CascadeRiskLevel::Deep);
        assert_eq!(ws[0].depth, 3);
        assert_eq!(ws[0].parent_table, "users");
    }

    #[rstest]
    fn case_03_high_fanout_no_depth_flagged() {
        // posts has 3 cascade children, depth still 1
        let baseline = vec![
            table_with_cascade_fks("users", vec!["id"], vec![]),
            table_with_cascade_fks("posts", vec!["id"], vec![]),
            table_with_cascade_fks("tags", vec!["id", "post_id"], vec![("post_id", "posts")]),
            table_with_cascade_fks("votes", vec!["id", "post_id"], vec![("post_id", "posts")]),
            table_with_cascade_fks("views", vec!["id", "post_id"], vec![("post_id", "posts")]),
        ];
        let p = plan(vec![add_cascade_fk("posts", "user_id", "users")]);
        let ws = find_cascade_reach_violations(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].risk_level, CascadeRiskLevel::HighFanout);
        assert_eq!(ws[0].max_fanout, 3);
    }

    #[rstest]
    fn case_04_critical_both_deep_and_fanout() {
        // users <- posts (fanout 3) <- comments <- reactions
        let baseline = vec![
            table_with_cascade_fks("users", vec!["id"], vec![]),
            table_with_cascade_fks("posts", vec!["id"], vec![]),
            table_with_cascade_fks(
                "comments",
                vec!["id", "post_id"],
                vec![("post_id", "posts")],
            ),
            table_with_cascade_fks("tags", vec!["id", "post_id"], vec![("post_id", "posts")]),
            table_with_cascade_fks("votes", vec!["id", "post_id"], vec![("post_id", "posts")]),
            table_with_cascade_fks(
                "reactions",
                vec!["id", "comment_id"],
                vec![("comment_id", "comments")],
            ),
            table_with_cascade_fks(
                "notifications",
                vec!["id", "reaction_id"],
                vec![("reaction_id", "reactions")],
            ),
        ];
        let p = plan(vec![add_cascade_fk("posts", "user_id", "users")]);
        let ws = find_cascade_reach_violations(&p, &baseline);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].risk_level, CascadeRiskLevel::Critical);
    }

    #[rstest]
    fn case_05_self_referential_no_warning() {
        // categories.parent_id -> categories.id - tree shape, cycle
        // detection prevents infinite loop and the depth from a
        // self-referential CASCADE FK is bounded by the visited set
        // (single hop).
        let baseline = vec![table_with_cascade_fks(
            "categories",
            vec!["id", "parent_id"],
            vec![],
        )];
        let p = plan(vec![add_cascade_fk(
            "categories",
            "parent_id",
            "categories",
        )]);
        let ws = find_cascade_reach_violations(&p, &baseline);
        // self-referential cascade is naturally depth-1 only (the
        // visited set blocks re-entry), so no warning.
        assert!(ws.is_empty());
    }

    #[rstest]
    fn case_06_restrict_fk_not_in_graph() {
        // posts uses RESTRICT, downstream chain via CASCADE -
        // because the *added* FK is RESTRICT, F96 ignores it.
        let baseline = vec![
            table_with_cascade_fks("users", vec!["id"], vec![]),
            table_with_cascade_fks("posts", vec!["id", "user_id"], vec![]),
            table_with_cascade_fks(
                "comments",
                vec!["id", "post_id"],
                vec![("post_id", "posts")],
            ),
            table_with_cascade_fks(
                "reactions",
                vec!["id", "comment_id"],
                vec![("comment_id", "comments")],
            ),
        ];
        let p = plan(vec![add_restrict_fk("posts", "user_id", "users")]);
        assert!(find_cascade_reach_violations(&p, &baseline).is_empty());
    }

    #[rstest]
    fn case_07_cycle_does_not_loop() {
        // A -> B -> C -> A cycle (theoretical, malformed but
        // possible). DFS must terminate.
        let baseline = vec![
            table_with_cascade_fks("a", vec!["id", "c_id"], vec![("c_id", "c")]),
            table_with_cascade_fks("b", vec!["id", "a_id"], vec![("a_id", "a")]),
            table_with_cascade_fks("c", vec!["id", "b_id"], vec![("b_id", "b")]),
        ];
        // Adding any new FK does not change the analysis from a
        // termination perspective; just verify it doesn't hang.
        let p = plan(vec![add_cascade_fk("a", "c_id", "c")]);
        let _ = find_cascade_reach_violations(&p, &baseline);
    }

    #[rstest]
    fn case_08_multiple_new_cascade_fks_each_evaluated() {
        let baseline = vec![
            table_with_cascade_fks("users", vec!["id"], vec![]),
            table_with_cascade_fks("posts", vec!["id"], vec![]),
            table_with_cascade_fks(
                "comments",
                vec!["id", "post_id"],
                vec![("post_id", "posts")],
            ),
            table_with_cascade_fks(
                "reactions",
                vec!["id", "comment_id"],
                vec![("comment_id", "comments")],
            ),
        ];
        // Add two new cascade FKs: one shallow (users -> posts, but
        // continues to depth 3) and one shallow (orphan)
        let p = plan(vec![
            add_cascade_fk("posts", "user_id", "users"),
            add_cascade_fk("orphan", "x_id", "ghost"),
        ]);
        let ws = find_cascade_reach_violations(&p, &baseline);
        // Only the first triggers a deep warning; the second goes
        // to a non-existent parent, depth 0, no warning.
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].origin_child_table, "posts");
    }
}
