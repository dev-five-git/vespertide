//! Box / edge data model and builders for the SVG ERD renderer.
//!
//! Owns the in-memory representation of tables and FK edges, plus the
//! measurement logic that decides how wide each table card should be.

// SVG layout converts integer column / row / badge counts into pixel widths.
// The casts are bounded by the model itself and add noise without catching
// real bugs.
#![expect(
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    reason = "SVG layout converts bounded table/row counts into pixel coordinates"
)]

use std::collections::BTreeMap;

use vespertide_core::{ColumnDef, TableDef};

use super::super::{ForeignKeyRelation, is_foreign_key_column, is_primary_key_column};
use super::style::{
    BADGE_GAP, BADGE_W, COL_GAP_TYPE, HEADER_H, NAME_CH, ROW_H, TABLE_PAD_X, TITLE_CH, TYPE_CH,
};

#[derive(Debug, Clone)]
pub(super) struct TableBox {
    pub(super) name: String,
    pub(super) rows: Vec<RowSpec>,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) x: f64,
    pub(super) y: f64,
    /// Column-name → row index, for fast FK row lookup.
    pub(super) row_index: BTreeMap<String, usize>,
    /// First PK row index, used as anchor for incoming edges.
    pub(super) pk_row: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct RowSpec {
    pub(super) name: String,
    pub(super) type_str: String,
    pub(super) is_pk: bool,
    pub(super) is_fk: bool,
    pub(super) nullable: bool,
}

#[derive(Debug, Clone)]
pub(super) struct EdgeSpec {
    pub(super) child_idx: usize,
    pub(super) parent_idx: usize,
    pub(super) child_row: usize,
    pub(super) parent_row: usize,
    pub(super) label: String,
    pub(super) cardinality_label: &'static str,
    /// 0-based index among parallel edges sharing the same (child, parent)
    /// unordered pair. Used to spread cardinality labels along the curve.
    pub(super) parallel_index: u32,
    /// Total number of parallel edges in the same group.
    pub(super) parallel_count: u32,
}

pub(super) fn build_boxes(tables: &[TableDef]) -> Vec<TableBox> {
    tables
        .iter()
        .map(|table| {
            let rows: Vec<RowSpec> = table
                .columns
                .iter()
                .map(|column| build_row(table, column))
                .collect();

            let mut row_index = BTreeMap::new();
            let mut pk_row = None;
            for (idx, row) in rows.iter().enumerate() {
                row_index.insert(row.name.clone(), idx);
                if row.is_pk && pk_row.is_none() {
                    pk_row = Some(idx);
                }
            }

            let width = measure_table_width(&table.name, &rows);
            let height = HEADER_H + ROW_H * rows.len() as f64;

            TableBox {
                name: table.name.to_string(),
                rows,
                width,
                height,
                x: 0.0,
                y: 0.0,
                row_index,
                pk_row,
            }
        })
        .collect()
}

fn build_row(table: &TableDef, column: &ColumnDef) -> RowSpec {
    RowSpec {
        name: column.name.to_string(),
        type_str: column.r#type.to_display_string(),
        is_pk: is_primary_key_column(table, &column.name),
        is_fk: is_foreign_key_column(table, &column.name),
        nullable: column.nullable,
    }
}

fn measure_table_width(name: &str, rows: &[RowSpec]) -> f64 {
    let title_w = name.chars().count() as f64 * TITLE_CH + TABLE_PAD_X * 2.0;

    let row_max = rows
        .iter()
        .map(|row| {
            let badges = badge_block_width(row);
            let name_w = row.name.chars().count() as f64 * NAME_CH;
            let type_w = row.type_str.chars().count() as f64 * TYPE_CH;
            TABLE_PAD_X * 2.0 + badges + name_w + COL_GAP_TYPE + type_w
        })
        .fold(0.0_f64, f64::max);

    let raw = title_w.max(row_max).max(180.0);
    // Round up to a nice 4-pixel grid for crispness.
    (raw / 4.0).ceil() * 4.0
}

fn badge_block_width(row: &RowSpec) -> f64 {
    let mut count = 0;
    if row.is_pk {
        count += 1;
    }
    if row.is_fk {
        count += 1;
    }
    if count == 0 {
        return 0.0;
    }
    count as f64 * BADGE_W + (count as f64 - 1.0).max(0.0) * 4.0 + BADGE_GAP
}

pub(super) fn build_edges(
    tables: &[TableDef],
    boxes: &[TableBox],
    relations: &std::collections::BTreeSet<ForeignKeyRelation>,
) -> Vec<EdgeSpec> {
    let name_idx: BTreeMap<&str, usize> = tables
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.as_str(), i))
        .collect();

    let mut edges = Vec::new();
    for rel in relations {
        let Some(&child_idx) = name_idx.get(rel.child_table.as_str()) else {
            continue;
        };
        let Some(&parent_idx) = name_idx.get(rel.parent_table.as_str()) else {
            continue;
        };
        if child_idx == parent_idx {
            // Self-reference: skip drawing (rare and hard to route nicely).
            continue;
        }

        let child_row = rel
            .child_columns
            .first()
            .and_then(|c| boxes[child_idx].row_index.get(c).copied())
            .unwrap_or(0);
        let parent_row = rel
            .parent_columns
            .first()
            .and_then(|c| boxes[parent_idx].row_index.get(c).copied())
            .or(boxes[parent_idx].pk_row)
            .unwrap_or(0);

        let label = format!(
            "{} → {}",
            rel.child_columns.join(", "),
            rel.parent_columns.join(", ")
        );

        edges.push(EdgeSpec {
            child_idx,
            parent_idx,
            child_row,
            parent_row,
            label,
            cardinality_label: rel.cardinality.label(),
            parallel_index: 0,
            parallel_count: 1,
        });
    }

    // Group parallel edges sharing the same unordered (child, parent) pair so
    // labels and curves can be spread along the bundle instead of stacking.
    let mut group_map: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for (i, edge) in edges.iter().enumerate() {
        let lo = edge.child_idx.min(edge.parent_idx);
        let hi = edge.child_idx.max(edge.parent_idx);
        group_map.entry((lo, hi)).or_default().push(i);
    }
    for indices in group_map.values() {
        let count = u32::try_from(indices.len()).unwrap_or(1);
        for (slot, &edge_idx) in indices.iter().enumerate() {
            let parallel_index = u32::try_from(slot).unwrap_or(0);
            edges[edge_idx].parallel_index = parallel_index;
            edges[edge_idx].parallel_count = count;
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    //! Defensive-arm coverage for `build_edges`. The normal `collect_foreign_key_relations`
    //! pipeline never feeds in a relation whose endpoints are missing from
    //! `tables`, but the unknown-child / unknown-parent / self-reference arms
    //! exist anyway. We poke them via hand-crafted `ForeignKeyRelation` sets.
    use super::super::super::{Cardinality, ForeignKeyRelation};
    use super::*;
    use std::collections::BTreeSet;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    fn solo_table() -> TableDef {
        TableDef {
            name: "solo".into(),
            description: None,
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                    .primary_key(PrimaryKeySyntax::Bool(true)),
            ],
            constraints: vec![],
        }
    }

    fn rel(child: &str, parent: &str) -> ForeignKeyRelation {
        ForeignKeyRelation {
            child_table: child.into(),
            child_columns: vec!["x".into()],
            parent_table: parent.into(),
            parent_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            cardinality: Cardinality::OneToMany,
        }
    }

    // A 30-char title with no rows makes title width dominate (> the 180px
    // floor), so the exact value pins every arm of
    // `chars * TITLE_CH + TABLE_PAD_X * 2.0`: 30*7.9 + 28 = 265 -> rounded up
    // to the 4px grid = 268. Each `*`/`+` mutant changes the result.
    #[test]
    fn measure_table_width_title_dominates_exact_value() {
        assert_eq!(measure_table_width(&"x".repeat(30), &[]), 268.0);
    }

    // A table with NO primary key must have pk_row == None. Pins
    // `row.is_pk && pk_row.is_none()`: a `||` mutant fires on the FIRST row
    // (because pk_row is still None) and sets pk_row = Some(0) even though no
    // column is a PK. (With a real PK present, the loop's later overwrite hides
    // the mutation, so a PK-less table is the distinguishing case.)
    #[test]
    fn build_boxes_pk_row_is_none_when_no_primary_key() {
        let table = TableDef {
            name: "t".into(),
            description: None,
            columns: vec![
                ColumnDef::new("a", ColumnType::Simple(SimpleColumnType::Text), true),
                ColumnDef::new("b", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            constraints: vec![],
        };
        let boxes = build_boxes(&[table]);
        assert_eq!(boxes[0].pk_row, None);
    }

    #[test]
    fn build_edges_skips_unknown_child_endpoint() {
        // model.rs:120-121 — child_table not in tables -> early continue.
        let tables = vec![solo_table().normalize().unwrap()];
        let boxes = build_boxes(&tables);
        let mut rels = BTreeSet::new();
        rels.insert(rel("ghost", "solo"));
        let edges = build_edges(&tables, &boxes, &rels);
        assert!(edges.is_empty());
    }

    #[test]
    fn build_edges_skips_unknown_parent_endpoint() {
        // model.rs:123-124 — parent_table not in tables -> early continue.
        let tables = vec![solo_table().normalize().unwrap()];
        let boxes = build_boxes(&tables);
        let mut rels = BTreeSet::new();
        rels.insert(rel("solo", "ghost"));
        let edges = build_edges(&tables, &boxes, &rels);
        assert!(edges.is_empty());
    }

    #[test]
    fn build_edges_skips_self_referencing_relation() {
        // model.rs:126-128 — self-reference (child_idx == parent_idx) skipped.
        let tables = vec![solo_table().normalize().unwrap()];
        let boxes = build_boxes(&tables);
        let mut rels = BTreeSet::new();
        rels.insert(rel("solo", "solo"));
        let edges = build_edges(&tables, &boxes, &rels);
        assert!(edges.is_empty());
    }
}
