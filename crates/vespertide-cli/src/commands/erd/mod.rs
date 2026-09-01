pub mod dot;
pub mod mermaid;
pub mod svg;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::ValueEnum;
use vespertide_core::schema::foreign_key::ForeignKeySyntax;
use vespertide_core::schema::names::names_to_strings;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ReferenceAction, StrOrBoolOrArray, TableConstraint, TableDef};

use crate::utils::{load_config, load_models};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ErdFormat {
    Svg,
    Mermaid,
    Dot,
}

pub async fn cmd_erd_with_filters(
    format: ErdFormat,
    output: Option<PathBuf>,
    include: Vec<String>,
    exclude: Vec<String>,
    depth: usize,
) -> Result<()> {
    let config = load_config()?;
    let tables = filter_tables(
        normalize_tables(load_models(&config)?)?,
        &include,
        &exclude,
        depth,
    );

    let rendered = match format {
        ErdFormat::Svg => svg::render_svg(&tables).map_err(anyhow::Error::msg)?,
        ErdFormat::Mermaid => mermaid::render_mermaid(&tables),
        ErdFormat::Dot => dot::render_dot(&tables),
    };

    if let Some(path) = output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create ERD output directory {}", parent.display()))?;
        }

        tokio::fs::write(&path, rendered)
            .await
            .with_context(|| format!("write ERD output {}", path.display()))?;
        println!("ERD exported to {}", path.display());
    } else {
        print!("{rendered}");
    }

    Ok(())
}

#[expect(
    clippy::print_stderr,
    reason = "ERD filter warnings are user-facing diagnostics and must not mix with generated diagram stdout"
)]
pub(super) fn filter_tables(
    tables: Vec<TableDef>,
    include: &[String],
    exclude: &[String],
    depth: usize,
) -> Vec<TableDef> {
    let (tables, warnings) = filter_tables_with_warnings(tables, include, exclude, depth);
    for warning in warnings {
        eprintln!("{warning}");
    }
    tables
}

pub(super) fn filter_tables_with_warnings(
    tables: Vec<TableDef>,
    include: &[String],
    exclude: &[String],
    depth: usize,
) -> (Vec<TableDef>, Vec<String>) {
    if include.is_empty() && exclude.is_empty() {
        return (tables, Vec::new());
    }

    let include = normalized_filter_names(include);
    let exclude = normalized_filter_names(exclude);
    let all_names: BTreeSet<String> = tables.iter().map(|table| table.name.to_string()).collect();

    let mut warnings = filter_warnings(&all_names, "--include", &include);
    warnings.extend(filter_warnings(&all_names, "--exclude", &exclude));

    let mut kept: BTreeSet<String> = if include.is_empty() {
        all_names.clone()
    } else {
        include
            .iter()
            .filter(|name| all_names.contains(*name))
            .cloned()
            .collect()
    };

    let adjacency = build_fk_adjacency(&tables);
    // Layered BFS: each round expands only the names discovered in the
    // previous round instead of re-scanning (and re-cloning) all of `kept`,
    // so every node's adjacency is walked at most once regardless of depth.
    // The discovered-neighbor frontier borrows `&str` into the owned `String`s
    // that live in `adjacency`, so only the `kept` set membership needs an
    // owned clone (the frontier itself no longer clones every neighbor name).
    // The seed must be a snapshot detached from `kept` so the frontier borrow
    // does not collide with the `kept.insert` below; `seed_names` owns it.
    let seed_names: Vec<String> = kept.iter().cloned().collect();
    let mut frontier: Vec<&str> = seed_names.iter().map(String::as_str).collect();
    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let mut next: Vec<&str> = Vec::new();
        for name in frontier {
            if let Some(neighbors) = adjacency.get(name) {
                for neighbor in neighbors {
                    if all_names.contains(neighbor) && kept.insert(neighbor.clone()) {
                        next.push(neighbor.as_str());
                    }
                }
            }
        }
        frontier = next;
    }

    for name in exclude {
        kept.remove(&name);
    }

    let filtered = tables
        .into_iter()
        .filter(|table| kept.contains(table.name.as_str()))
        .collect();
    (filtered, warnings)
}

fn normalize_tables(tables: Vec<TableDef>) -> Result<Vec<TableDef>> {
    tables
        .into_iter()
        .map(|table| {
            table
                .normalize()
                .with_context(|| format!("normalize table '{}'", table.name))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ForeignKeyRelation {
    pub child_table: String,
    pub child_columns: Vec<String>,
    pub parent_table: String,
    pub parent_columns: Vec<String>,
    pub on_delete: Option<ReferenceAction>,
    pub on_update: Option<ReferenceAction>,
    pub cardinality: Cardinality,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Cardinality {
    OneToOne,
    OneToMany,
    ZeroOrOneToMany,
    ManyToMany,
}

impl Cardinality {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::OneToOne => "1:1",
            Self::OneToMany => "1:N",
            Self::ZeroOrOneToMany => "0..1:N",
            Self::ManyToMany => "M:N",
        }
    }
}

pub(super) fn collect_foreign_key_relations(tables: &[TableDef]) -> BTreeSet<ForeignKeyRelation> {
    let mut relations = BTreeSet::new();
    let table_lookup: BTreeMap<&str, &TableDef> = tables
        .iter()
        .map(|table| (table.name.as_str(), table))
        .collect();

    for table in tables {
        for constraint in &table.constraints {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } = constraint
            {
                let Some(parent_table) = table_lookup.get(ref_table.as_str()).copied() else {
                    continue;
                };
                relations.insert(build_foreign_key_relation(
                    table,
                    names_to_strings(columns),
                    ref_table.to_string(),
                    names_to_strings(ref_columns),
                    on_delete.clone(),
                    on_update.clone(),
                    parent_table,
                ));
            }
        }

        for column in &table.columns {
            if let Some(foreign_key) = &column.foreign_key
                && let Some(relation) =
                    inline_foreign_key_relation(table, column, foreign_key, &table_lookup)
            {
                relations.insert(relation);
            }
        }
    }

    relations
}

pub(super) fn is_primary_key_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && is_inline_primary_key(column))
        || table.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                TableConstraint::PrimaryKey { columns, .. }
                    if columns.iter().any(|column| column == column_name)
            )
        })
}

pub(super) fn is_foreign_key_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && column.foreign_key.is_some())
        || table.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                TableConstraint::ForeignKey { columns, .. }
                    if columns.iter().any(|column| column == column_name)
            )
        })
}

pub(super) fn column_markers(table: &TableDef, column: &ColumnDef) -> String {
    // The marker set is at most two fixed strings ("PK", "FK"), so the output is
    // exactly one of four values. A direct match returning a &'static str for the
    // three non-empty cases removes the Vec + join + format! allocations.
    match (
        is_primary_key_column(table, &column.name),
        is_foreign_key_column(table, &column.name),
    ) {
        (true, true) => " (PK, FK)".to_string(),
        (true, false) => " (PK)".to_string(),
        (false, true) => " (FK)".to_string(),
        (false, false) => String::new(),
    }
}

fn inline_foreign_key_relation(
    table: &TableDef,
    column: &ColumnDef,
    foreign_key: &ForeignKeySyntax,
    table_lookup: &BTreeMap<&str, &TableDef>,
) -> Option<ForeignKeyRelation> {
    let (parent_table, parent_columns, on_delete, on_update) = match foreign_key {
        ForeignKeySyntax::String(reference) => {
            let (table, columns) = parse_reference(reference)?;
            (table, columns, None, None)
        }
        ForeignKeySyntax::Reference(reference) => {
            let (table, columns) = parse_reference(&reference.references)?;
            (
                table,
                columns,
                reference.on_delete.clone(),
                reference.on_update.clone(),
            )
        }
        ForeignKeySyntax::Object(definition) => (
            definition.ref_table.to_string(),
            names_to_strings(&definition.ref_columns),
            definition.on_delete.clone(),
            definition.on_update.clone(),
        ),
    };

    let parent_table_def = table_lookup.get(parent_table.as_str()).copied()?;
    Some(build_foreign_key_relation(
        table,
        vec![column.name.to_string()],
        parent_table,
        parent_columns,
        on_delete,
        on_update,
        parent_table_def,
    ))
}

fn parse_reference(reference: &str) -> Option<(String, Vec<String>)> {
    let mut parts = reference.split('.');
    let table = parts.next()?;
    let column = parts.next()?;

    if parts.next().is_some() || table.is_empty() || column.is_empty() {
        return None;
    }

    Some((table.to_string(), vec![column.to_string()]))
}

fn build_foreign_key_relation(
    child_table: &TableDef,
    child_columns: Vec<String>,
    parent_table: String,
    parent_columns: Vec<String>,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
    parent_table_def: &TableDef,
) -> ForeignKeyRelation {
    let cardinality = detect_cardinality(child_table, &child_columns, parent_table_def);
    ForeignKeyRelation {
        child_table: child_table.name.to_string(),
        child_columns,
        parent_table,
        parent_columns,
        on_delete,
        on_update,
        cardinality,
    }
}

fn detect_cardinality(
    child_table: &TableDef,
    child_columns: &[String],
    _parent_table: &TableDef,
) -> Cardinality {
    if is_junction_table(child_table) {
        return Cardinality::ManyToMany;
    }

    if are_columns_unique(child_table, child_columns) {
        return Cardinality::OneToOne;
    }

    if child_columns
        .iter()
        .any(|column| is_nullable_column(child_table, column))
    {
        return Cardinality::ZeroOrOneToMany;
    }

    Cardinality::OneToMany
}

fn is_junction_table(table: &TableDef) -> bool {
    let primary_key_columns = primary_key_columns(table);
    if primary_key_columns.len() < 2 {
        return false;
    }

    let foreign_key_groups = foreign_key_column_groups(table);
    if foreign_key_groups.len() < 2 {
        return false;
    }

    let foreign_key_columns: BTreeSet<&str> = foreign_key_groups
        .iter()
        .flat_map(|group| group.iter().map(String::as_str))
        .collect();

    primary_key_columns
        .iter()
        .all(|column| foreign_key_columns.contains(column.as_str()))
}

fn are_columns_unique(table: &TableDef, columns: &[String]) -> bool {
    if columns.is_empty() {
        return false;
    }

    let primary_key_columns = primary_key_columns(table);
    if !primary_key_columns.is_empty() && same_column_set(columns, &primary_key_columns) {
        return true;
    }

    table.constraints.iter().any(|constraint| {
        matches!(
            constraint,
            TableConstraint::Unique { columns: unique_columns, .. }
                if same_column_set(columns, unique_columns)
        )
    }) || inline_unique_column_groups(table)
        .iter()
        .any(|unique_columns| same_column_set(columns, unique_columns))
}

fn primary_key_columns(table: &TableDef) -> Vec<String> {
    if let Some(columns) = table.constraints.iter().find_map(|constraint| {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            Some(columns.as_slice())
        } else {
            None
        }
    }) {
        return names_to_strings(columns);
    }

    table
        .columns
        .iter()
        .filter(|column| is_inline_primary_key(column))
        .map(|column| column.name.to_string())
        .collect()
}

fn foreign_key_column_groups(table: &TableDef) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey { columns, .. } = constraint
            && !groups.iter().any(|group| same_column_set(group, columns))
        {
            groups.push(names_to_strings(columns));
        }
    }

    for column in &table.columns {
        if column.foreign_key.is_none() {
            continue;
        }
        let group = vec![column.name.to_string()];
        if !groups
            .iter()
            .any(|existing| same_column_set(existing, &group))
        {
            groups.push(group);
        }
    }

    groups
}

fn inline_unique_column_groups(table: &TableDef) -> Vec<Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for column in &table.columns {
        let Some(unique) = &column.unique else {
            continue;
        };

        match unique {
            StrOrBoolOrArray::Str(name) => {
                groups
                    .entry(name.clone())
                    .or_default()
                    .push(column.name.to_string());
            }
            StrOrBoolOrArray::Array(names) => {
                for name in names {
                    groups
                        .entry(name.clone())
                        .or_default()
                        .push(column.name.to_string());
                }
            }
            StrOrBoolOrArray::Bool(true) => {
                groups.insert(
                    format!("__auto_{}", column.name),
                    vec![column.name.to_string()],
                );
            }
            _ => {}
        }
    }
    groups.into_values().collect()
}

fn is_inline_primary_key(column: &ColumnDef) -> bool {
    matches!(
        &column.primary_key,
        Some(PrimaryKeySyntax::Bool(true) | PrimaryKeySyntax::Object(_))
    )
}

fn is_nullable_column(table: &TableDef, column_name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name == column_name && column.nullable)
}

fn same_column_set<T: AsRef<str>, U: AsRef<str>>(left: &[T], right: &[U]) -> bool {
    // Differing lengths can never yield equal sets, so skip building both
    // BTreeSets on the common mismatched-arity case (e.g. single-col vs composite).
    if left.len() != right.len() {
        return false;
    }
    let left: BTreeSet<&str> = left.iter().map(AsRef::as_ref).collect();
    let right: BTreeSet<&str> = right.iter().map(AsRef::as_ref).collect();
    left == right
}

fn normalized_filter_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter_map(|name| {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}

fn filter_warnings(
    all_names: &BTreeSet<String>,
    flag: &str,
    filter_names: &[String],
) -> Vec<String> {
    let unknown: BTreeSet<&str> = filter_names
        .iter()
        .map(String::as_str)
        .filter(|name| !all_names.contains(*name))
        .collect();

    unknown
        .into_iter()
        .map(|name| format!("warning: ERD {flag} references unknown table '{name}'"))
        .collect()
}

fn build_fk_adjacency(tables: &[TableDef]) -> BTreeMap<String, BTreeSet<String>> {
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = tables
        .iter()
        .map(|table| (table.name.to_string(), BTreeSet::new()))
        .collect();
    let mut junction_parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for relation in collect_foreign_key_relations(tables) {
        if let Some(neighbors) = adjacency.get_mut(&relation.child_table) {
            neighbors.insert(relation.parent_table.clone());
        }
        if let Some(neighbors) = adjacency.get_mut(&relation.parent_table) {
            neighbors.insert(relation.child_table.clone());
        }
        if relation.cardinality == Cardinality::ManyToMany {
            junction_parents
                .entry(relation.child_table)
                .or_default()
                .insert(relation.parent_table);
        }
    }

    for parents in junction_parents.values() {
        for parent in parents {
            for peer in parents {
                if parent != peer
                    && let Some(neighbors) = adjacency.get_mut(parent)
                {
                    neighbors.insert(peer.clone());
                }
            }
        }
    }

    adjacency
}

#[cfg(test)]
mod tests;
