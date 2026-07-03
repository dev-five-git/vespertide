//! Reverse-relation rendering (`has_one` / `has_many` / M2M via junction).
//!
//! For every table that references the current table by FK, decide whether
//! it produces a `has_one`, `has_many`, or `has_many ... via` relation, and
//! emit the corresponding `SeaORM` attribute and field.

use std::collections::{BTreeMap, HashMap, HashSet};

use vespertide_core::{TableConstraint, TableDef};

use super::super::imports::{
    resolve_relation_entity_module_path, sanitize_field_name, to_pascal_case, to_snake_case,
    unique_name,
};
use super::super::render::{primary_key_columns, single_column_unique_set};
use super::naming::{generate_relation_enum_name, pluralize, unique_relation_enum_name};

/// Information about a reverse relation to be generated.
struct ReverseRelation {
    /// Target entity name (the table that has FK to current table)
    target_entity: String,
    /// Whether it's `has_one` (true) or `has_many` (false)
    is_one_to_one: bool,
    /// Base field name before uniquification
    field_base: String,
    /// Base `relation_enum` name (from FK columns)
    base_relation_enum: String,
    /// Source table name (for disambiguation)
    source_table: String,
    /// Whether the source table has multiple FKs to current table
    has_multiple_fks: bool,
    /// Optional via clause for M2M relations
    via: Option<String>,
    /// Optional `via_rel` clause for reverse diamond relations
    via_rel: Option<String>,
    /// Whether this is a M2M relation (through junction table)
    is_m2m: bool,
}

/// Collect target entities from reverse relations (for counting across all relations).
pub(super) fn collect_reverse_relation_targets(
    table: &TableDef,
    schema: &[TableDef],
) -> Vec<String> {
    let mut targets = Vec::new();

    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        // Get PK columns for junction table detection
        let other_pk = primary_key_columns(other_table);

        // Check if this is a junction table
        if let Some(m2m_targets) =
            collect_many_to_many_targets(table, other_table, &other_pk, schema)
        {
            targets.extend(m2m_targets);
            continue;
        }

        // Check for direct FK to this table
        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                && ref_table == &table.name
            {
                targets.push(other_table.name.to_string());
            }
        }
    }

    targets
}

/// Collect target entities from a junction table for M2M relations.
fn collect_many_to_many_targets(
    current_table: &TableDef,
    junction_table: &TableDef,
    junction_pk: &HashSet<String>,
    schema: &[TableDef],
) -> Option<Vec<String>> {
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

    fks.iter()
        .find(|(_, ref_table)| ref_table == &current_table.name)?;

    let mut targets = Vec::new();

    // Junction table itself
    targets.push(junction_table.name.to_string());

    // Target tables via M2M
    for (_, ref_table) in &fks {
        if ref_table == &current_table.name {
            continue;
        }
        let target_exists = schema.iter().any(|t| &t.name == ref_table);
        if target_exists {
            targets.push(ref_table.to_string());
        }
    }

    Some(targets)
}

/// Generate reverse relation fields (`has_one/has_many`) for tables that reference this table.
pub(super) fn reverse_relation_field_defs(
    table: &TableDef,
    schema: &[TableDef],
    used: &mut HashSet<String>,
    entity_count: &BTreeMap<&str, usize>,
    used_relation_enums: &mut HashSet<String>,
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> Vec<String> {
    reverse_relation_field_defs_inner(ReverseRelationFieldCtx {
        table,
        schema,
        used,
        entity_count,
        used_relation_enums,
        module_paths,
        crate_prefix,
    })
}

struct ReverseRelationFieldCtx<'a> {
    table: &'a TableDef,
    schema: &'a [TableDef],
    used: &'a mut HashSet<String>,
    entity_count: &'a BTreeMap<&'a str, usize>,
    used_relation_enums: &'a mut HashSet<String>,
    module_paths: &'a HashMap<String, Vec<String>>,
    crate_prefix: &'a str,
}

fn reverse_relation_field_defs_inner(ctx: ReverseRelationFieldCtx<'_>) -> Vec<String> {
    let ReverseRelationFieldCtx {
        table,
        schema,
        used,
        entity_count,
        used_relation_enums,
        module_paths,
        crate_prefix,
    } = ctx;
    // First pass: collect all reverse relations
    let mut relations: Vec<ReverseRelation> = Vec::new();

    // Count how many FKs from each table reference this table
    let mut fk_count_per_table: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }
        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                && ref_table == &table.name
            {
                *fk_count_per_table
                    .entry(other_table.name.to_string())
                    .or_insert(0) += 1;
            }
        }
    }

    // Collect all relations from all tables
    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        // Get PK and unique columns for the other table
        let other_pk = primary_key_columns(other_table);
        let other_unique = single_column_unique_set(&other_table.constraints);

        // Check if this is a junction table (composite PK with multiple FKs)
        if let Some(m2m_relations) =
            collect_many_to_many_relations(table, other_table, &other_pk, schema)
        {
            relations.extend(m2m_relations);
            continue;
        }

        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = constraint
            {
                // Check if this FK references our table
                if ref_table == &table.name {
                    // Determine if it's has_one or has_many
                    let is_one_to_one = if columns.len() == 1 {
                        let col = &columns[0];
                        let is_sole_pk = other_pk.len() == 1 && other_pk.contains(col.as_str());
                        let is_unique = other_unique.contains(col.as_str());
                        is_sole_pk || is_unique
                    } else {
                        columns.len() == other_pk.len()
                            && columns.iter().all(|c| other_pk.contains(c.as_str()))
                    };

                    let has_multiple_fks = fk_count_per_table
                        .get(other_table.name.as_str())
                        .is_some_and(|count| *count > 1);

                    // Generate base field name
                    let base_relation_enum = generate_relation_enum_name(columns);
                    let field_base = if has_multiple_fks {
                        let lowercase_enum = to_snake_case(&base_relation_enum);
                        if is_one_to_one {
                            lowercase_enum
                        } else {
                            format!(
                                "{}_{}",
                                lowercase_enum,
                                pluralize(&sanitize_field_name(&other_table.name))
                            )
                        }
                    } else if is_one_to_one {
                        sanitize_field_name(&other_table.name)
                    } else {
                        pluralize(&sanitize_field_name(&other_table.name))
                    };

                    relations.push(ReverseRelation {
                        target_entity: other_table.name.to_string(),
                        is_one_to_one,
                        field_base,
                        base_relation_enum,
                        source_table: other_table.name.to_string(),
                        has_multiple_fks,
                        via: None,
                        via_rel: Some(generate_relation_enum_name(columns)),
                        is_m2m: false,
                    });
                }
            }
        }
    }

    // Second pass: generate output with relation_enum when needed
    let mut out = Vec::new();

    for rel in relations {
        let relation_type = if rel.is_one_to_one {
            "has_one"
        } else {
            "has_many"
        };
        let rust_type = if rel.is_one_to_one {
            "HasOne"
        } else {
            "HasMany"
        };
        let field_name = unique_name(&rel.field_base, used);

        // Determine if we need relation_enum:
        // 1. Multiple FKs from same source table, OR
        // 2. Multiple relations targeting the same entity (across ALL relations including forward)
        let needs_relation_enum = rel.has_multiple_fks
            || entity_count
                .get(rel.target_entity.as_str())
                .is_some_and(|c| *c > 1);

        let attr = if needs_relation_enum {
            let preferred_relation_enum_name = if rel.is_m2m {
                // M2M: use {Target}Via{Junction} pattern directly
                // e.g., "MediaViaUserMediaRole"
                rel.base_relation_enum.clone()
            } else {
                let via_value = rel.via.as_ref().unwrap_or(&rel.source_table);
                // Direct: use via table name, fall back to FK-based on collision
                let base_enum = to_pascal_case(via_value);
                if used_relation_enums.contains(&base_enum) {
                    rel.base_relation_enum.clone()
                } else {
                    base_enum
                }
            };
            let relation_enum_name = unique_relation_enum_name(
                preferred_relation_enum_name,
                &rel.source_table,
                &rel.base_relation_enum,
                used_relation_enums,
            );
            used_relation_enums.insert(relation_enum_name.clone());

            if let Some(via_rel) = &rel.via_rel {
                format!(
                    "    #[sea_orm({relation_type}, relation_enum = \"{relation_enum_name}\", via_rel = \"{via_rel}\")]"
                )
            } else if let Some(via) = &rel.via {
                format!(
                    "    #[sea_orm({relation_type}, relation_enum = \"{relation_enum_name}\", via = \"{via}\")]"
                )
            } else {
                format!("    #[sea_orm({relation_type}, relation_enum = \"{relation_enum_name}\")]")
            }
        } else if let Some(via) = &rel.via {
            // No ambiguity - just via without relation_enum
            format!("    #[sea_orm({relation_type}, via = \"{via}\")]")
        } else {
            format!("    #[sea_orm({relation_type})]")
        };

        out.push(attr);
        let entity_path = resolve_relation_entity_module_path(
            &table.name,
            &rel.target_entity,
            module_paths,
            crate_prefix,
        );
        out.push(format!(
            "    pub {field_name}: {rust_type}<{entity_path}::Entity>,"
        ));
    }

    out
}

/// Collect many-to-many relations from a junction table.
/// Returns Some(relations) if it's a junction table that links current table to other tables,
/// or None if it's not a junction table.
fn collect_many_to_many_relations(
    current_table: &TableDef,
    junction_table: &TableDef,
    junction_pk: &HashSet<String>,
    schema: &[TableDef],
) -> Option<Vec<ReverseRelation>> {
    // Junction table must have composite PK (2+ columns)
    if junction_pk.len() < 2 {
        return None;
    }

    // Collect all FKs from the junction table
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

    // Must have at least 2 FKs to be a junction table
    if fks.len() < 2 {
        return None;
    }

    // Check if all FK columns are part of the PK (typical junction table pattern)
    let all_fk_cols_in_pk = fks
        .iter()
        .all(|(cols, _)| cols.iter().all(|c| junction_pk.contains(c.as_str())));

    if !all_fk_cols_in_pk {
        return None;
    }

    // Find which FK references the current table
    fks.iter()
        .find(|(_, ref_table)| ref_table == &current_table.name)?;

    let mut relations = Vec::new();

    let self_ref_fks: Vec<_> = fks
        .iter()
        .filter(|(_, ref_table)| ref_table == &current_table.name)
        .cloned()
        .collect();

    if self_ref_fks.len() == fks.len() {
        return None;
    }

    // First, add has_many to the junction table itself (direct relation, not M2M)
    let junction_base = pluralize(&sanitize_field_name(&junction_table.name));
    relations.push(ReverseRelation {
        target_entity: junction_table.name.to_string(),
        is_one_to_one: false,
        field_base: junction_base,
        base_relation_enum: to_pascal_case(&junction_table.name),
        source_table: junction_table.name.to_string(),
        has_multiple_fks: false,
        via: None,
        via_rel: None,
        is_m2m: false,
    });

    // Then add has_many with via for the target tables (M2M relations)
    for (_columns, ref_table) in &fks {
        if ref_table == &current_table.name {
            continue;
        }

        let target_exists = schema.iter().any(|t| &t.name == ref_table);
        if !target_exists {
            continue;
        }

        let field_base = format!(
            "{}_via_{}",
            pluralize(&sanitize_field_name(ref_table)),
            sanitize_field_name(&junction_table.name)
        );
        let base_relation_enum = format!(
            "{}Via{}",
            to_pascal_case(ref_table),
            to_pascal_case(&junction_table.name)
        );

        relations.push(ReverseRelation {
            target_entity: ref_table.to_string(),
            is_one_to_one: false,
            field_base,
            base_relation_enum,
            source_table: junction_table.name.to_string(),
            has_multiple_fks: false,
            via: Some(junction_table.name.to_string()),
            via_rel: None,
            is_m2m: true,
        });
    }

    Some(relations)
}
