use vespertide_core::TableDef;
use vespertide_core::schema::constraint::TableConstraint;

pub(crate) struct CompositeFk<'a> {
    pub local_cols: Vec<&'a str>,
    pub ref_table: &'a str,
    pub ref_cols: Vec<&'a str>,
}

pub(crate) fn collect_composite_fks(table: &TableDef) -> Vec<CompositeFk<'_>> {
    table
        .constraints
        .iter()
        .filter_map(|constraint| match constraint {
            TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                ..
            } if columns.len() > 1 && columns.len() == ref_columns.len() => Some(CompositeFk {
                local_cols: columns.iter().map(AsRef::as_ref).collect(),
                ref_table: ref_table.as_str(),
                ref_cols: ref_columns.iter().map(AsRef::as_ref).collect(),
            }),
            _ => None,
        })
        .collect()
}
