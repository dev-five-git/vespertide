//! `SeaORM` relation rendering.
//!
//! Splits across four sub-modules:
//!
//! - [`fk_resolve`]: FK→FK chain resolution so relations always render against
//!   the ultimate target table, not an intermediate pass-through.
//! - [`naming`]: Pure naming helpers — relation-enum disambiguation, field-name
//!   inference, pluralization, FK attribute rendering.
//! - [`self_ref`]: Self-referencing junction tables (`Linked` impls +
//!   `find_*` query helpers).
//! - [`reverse`]: `has_one` / `has_many` / M2M-via reverse relation rendering.
//!
//! This module owns the forward (`belongs_to`) relation rendering entry point
//! and re-exports the items needed by `seaorm::render` and the test glob in
//! `seaorm::mod`. Items in sub-modules that need cross-`seaorm` visibility
//! are scoped with `pub(in crate::seaorm)`, preserving the visibility
//! envelope of the pre-split `relations.rs` where every public item was
//! `pub(super)` from `seaorm`.

use std::collections::{BTreeMap, HashMap, HashSet};

use vespertide_core::TableDef;

use super::imports::{
    resolve_relation_entity_module_path, sanitize_field_name, sanitize_type_name, to_pascal_case,
    unique_name,
};

mod fk_resolve;
mod naming;
mod reverse;
mod self_ref;

use fk_resolve::resolve_table_fks_pure;
use naming::fk_attr_value;
use reverse::{collect_reverse_relation_targets, reverse_relation_field_defs};

// The `pub(in crate::seaorm) use` form simultaneously binds the items in this
// module's scope (so `relation_field_defs_with_schema` can call them
// unqualified) AND re-exports them at the `relations` namespace, satisfying
// the test glob `#[cfg(test)] use relations::*;` in `seaorm/mod.rs` and the
// explicit `use super::relations::{...}` in `seaorm/render.rs`.
pub(in crate::seaorm) use naming::{generate_relation_enum_name, infer_field_name_from_fk_column};
pub(in crate::seaorm) use self_ref::{render_self_ref_link_helpers, render_self_ref_query_helpers};

// Additional helpers that only the seaorm-level test glob consumes; gated
// with `cfg(test)` so production builds don't carry unused re-exports.
#[cfg(test)]
pub(in crate::seaorm) use fk_resolve::resolve_fk_target;
#[cfg(test)]
pub(in crate::seaorm) use naming::{pluralize, unique_relation_enum_name};
#[cfg(test)]
pub(in crate::seaorm) use self_ref::resolve_self_ref_link_module_path;

pub(in crate::seaorm) fn relation_field_defs_with_schema(
    table: &TableDef,
    schema: &[TableDef],
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    // Relation field names are derived from column and table names, so they can
    // land on a name a column already took. Claiming every column up front makes
    // the relation take the numbered variant instead of redeclaring the field.
    let mut used: HashSet<String> = table
        .columns
        .iter()
        .map(|column| sanitize_field_name(&column.name))
        .collect();
    let forward_relations = resolve_table_fks_pure(table, schema);
    let mut all_target_entities: Vec<String> = forward_relations
        .iter()
        .map(|relation| relation.resolved_table.to_string())
        .collect();
    let reverse_targets = collect_reverse_relation_targets(table, schema);
    all_target_entities.extend(reverse_targets);
    let mut entity_count: BTreeMap<&str, usize> = BTreeMap::new();
    for entity in &all_target_entities {
        *entity_count.entry(entity.as_str()).or_insert(0) += 1;
    }
    let mut fk_by_table: BTreeMap<&str, usize> = BTreeMap::new();
    for relation in &forward_relations {
        *fk_by_table.entry(relation.resolved_table).or_insert(0) += 1;
    }
    let mut used_relation_enums: HashSet<String> = HashSet::new();
    for relation in &forward_relations {
        let columns = relation.columns;
        let resolved_table = relation.resolved_table;
        let resolved_columns = &relation.resolved_columns;

        let from = fk_attr_value(columns);
        let to = fk_attr_value(resolved_columns);

        let fks_to_this_table = fk_by_table.get(resolved_table).copied().unwrap_or(0);

        let entity_appears_multiple_times =
            entity_count.get(resolved_table).is_some_and(|c| *c > 1);

        // Inference works on the database names, so it gets the referenced
        // column rather than the escaped field name `to` carries.
        let field_base = if columns.len() == 1 {
            let referenced = resolved_columns.first().map_or("", AsRef::as_ref);
            infer_field_name_from_fk_column(&columns[0], resolved_table, referenced)
        } else {
            sanitize_field_name(resolved_table)
        };

        let field_name = unique_name(&field_base, &mut used);

        let needs_relation_enum = fks_to_this_table > 1 || entity_appears_multiple_times;

        let attr = if needs_relation_enum {
            let base_relation_enum = generate_relation_enum_name(columns);
            let relation_enum_name = if used_relation_enums.contains(&base_relation_enum) {
                format!(
                    "{base_relation_enum}{}",
                    sanitize_type_name(&to_pascal_case(&table.name))
                )
            } else {
                base_relation_enum.clone()
            };
            used_relation_enums.insert(relation_enum_name.clone());
            format!(
                "    #[sea_orm(belongs_to, relation_enum = \"{relation_enum_name}\", from = \"{from}\", to = \"{to}\")]"
            )
        } else {
            format!("    #[sea_orm(belongs_to, from = \"{from}\", to = \"{to}\")]")
        };

        out.push(attr);
        let entity_path = resolve_relation_entity_module_path(
            &table.name,
            resolved_table,
            module_paths,
            crate_prefix,
        );
        out.push(format!(
            "    pub {field_name}: HasOne<{entity_path}::Entity>,"
        ));
    }

    let reverse_relations = reverse_relation_field_defs(
        table,
        schema,
        &mut used,
        &entity_count,
        &mut used_relation_enums,
        module_paths,
        crate_prefix,
    );
    out.extend(reverse_relations);

    out
}
