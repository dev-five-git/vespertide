//! Self-referencing junction-table support.
//!
//! When a table participates in a junction whose two FK roles both point back
//! at the same target table (e.g. `friendship(user_a, user_b)`), we emit
//! `Linked` helpers and corresponding `Model::find_*` query methods so users
//! can traverse the relation in either direction.

use std::collections::{HashMap, HashSet};

use vespertide_core::{TableConstraint, TableDef};

use super::super::imports::{
    absolute_module_path, sanitize_field_name, to_pascal_case, unique_name,
};
use super::super::render::primary_key_columns;
use super::naming::{generate_relation_enum_name, pluralize};

pub(super) struct SelfRefJunction {
    pub(super) junction_table: String,
    pub(super) role_columns: Vec<String>,
    pub(super) role_relations: Vec<String>,
}

pub(super) fn collect_self_ref_junction(
    current_table: &TableDef,
    junction_table: &TableDef,
    junction_pk: &HashSet<String>,
) -> Option<SelfRefJunction> {
    if junction_pk.len() < 2 {
        return None;
    }

    let fks: Vec<_> = junction_table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = c
            {
                Some((columns.clone(), ref_table.clone()))
            } else {
                None
            }
        })
        .collect();

    if fks.len() < 2 {
        return None;
    }

    let all_fk_cols_in_pk = fks
        .iter()
        .all(|(cols, _)| cols.iter().all(|c| junction_pk.contains(c.as_str())));
    if !all_fk_cols_in_pk {
        return None;
    }

    if !fks
        .iter()
        .all(|(_, ref_table)| ref_table == &current_table.name)
    {
        return None;
    }

    Some(SelfRefJunction {
        junction_table: junction_table.name.to_string(),
        role_columns: fks.iter().map(|(cols, _)| cols[0].to_string()).collect(),
        role_relations: fks
            .iter()
            .map(|(cols, _)| generate_relation_enum_name(cols))
            .collect(),
    })
}

pub(super) fn self_ref_link_name(
    self_ref_junction: &SelfRefJunction,
    from_idx: usize,
    to_idx: usize,
) -> String {
    format!(
        "{}To{}Via{}",
        to_pascal_case(&self_ref_junction.role_columns[from_idx]),
        to_pascal_case(&self_ref_junction.role_columns[to_idx]),
        to_pascal_case(&self_ref_junction.junction_table)
    )
}

pub(in crate::seaorm) fn resolve_self_ref_link_module_path(
    current_table: &str,
    junction_table: &str,
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> String {
    if let (Some(current), Some(target)) = (
        module_paths.get(current_table),
        module_paths.get(junction_table),
    ) {
        let current_parent = current.split_last().map_or(&[][..], |(_, parent)| parent);
        let target_parent = target.split_last().map_or(&[][..], |(_, parent)| parent);

        if current_parent == target_parent {
            return format!("super::{junction_table}");
        }

        if !crate_prefix.is_empty() {
            return absolute_module_path(crate_prefix, target);
        }

        return absolute_module_path("crate::models", target);
    }

    format!("super::{junction_table}")
}

pub(in crate::seaorm) fn render_self_ref_link_helpers(
    table: &TableDef,
    schema: &[TableDef],
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> Vec<String> {
    let mut out = Vec::new();

    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        let other_pk = primary_key_columns(other_table);
        let Some(self_ref_junction) = collect_self_ref_junction(table, other_table, &other_pk)
        else {
            continue;
        };

        let junction_entity_path = resolve_self_ref_link_module_path(
            &table.name,
            &self_ref_junction.junction_table,
            module_paths,
            crate_prefix,
        );

        for (from_idx, from_role) in self_ref_junction.role_relations.iter().enumerate() {
            for (to_idx, to_role) in self_ref_junction.role_relations.iter().enumerate() {
                if from_idx == to_idx {
                    continue;
                }

                let link_name = self_ref_link_name(&self_ref_junction, from_idx, to_idx);
                out.push(format!("pub struct {link_name};"));
                out.push(format!("impl Linked for {link_name} {{"));
                out.push("    type FromEntity = Entity;".into());
                out.push("    type ToEntity = Entity;".into());
                out.push(String::new());
                out.push("    fn link(&self) -> Vec<RelationDef> {".into());
                out.push("        vec![".into());
                out.push(format!(
                    "            {junction_entity_path}::Relation::{from_role}.def().rev(),"
                ));
                out.push(format!(
                    "            {junction_entity_path}::Relation::{to_role}.def(),"
                ));
                out.push("        ]".into());
                out.push("    }".into());
                out.push("}".into());
                out.push(String::new());
            }
        }
    }

    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }

    out
}

pub(in crate::seaorm) fn render_self_ref_query_helpers(
    table: &TableDef,
    schema: &[TableDef],
) -> Vec<String> {
    let mut methods = Vec::new();
    let mut used_method_names = HashSet::new();

    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        let other_pk = primary_key_columns(other_table);
        let Some(self_ref_junction) = collect_self_ref_junction(table, other_table, &other_pk)
        else {
            continue;
        };

        for (from_idx, from_col) in self_ref_junction.role_columns.iter().enumerate() {
            for (to_idx, to_col) in self_ref_junction.role_columns.iter().enumerate() {
                if from_idx == to_idx {
                    continue;
                }

                let link_name = self_ref_link_name(&self_ref_junction, from_idx, to_idx);
                let method_base = format!(
                    "find_{}_via_{}_from_{}",
                    pluralize(&sanitize_field_name(to_col)),
                    sanitize_field_name(&self_ref_junction.junction_table),
                    sanitize_field_name(from_col)
                );
                let method_name = unique_name(&method_base, &mut used_method_names);

                methods.push(format!(
                    "    pub fn {method_name}(&self) -> Select<Entity> {{"
                ));
                methods.push(format!("        self.find_linked({link_name})"));
                methods.push("    }".into());
                methods.push(String::new());
            }
        }
    }

    while methods.last().is_some_and(String::is_empty) {
        methods.pop();
    }

    if methods.is_empty() {
        return methods;
    }

    let mut out = Vec::new();
    out.push("impl Model {".into());
    out.extend(methods);
    out.push("}".into());
    out
}
