use sea_query::{Alias, Index};

use super::super::types::BuiltQuery;

pub(super) fn build_index<T: AsRef<str>>(
    table: &str,
    name: Option<&str>,
    columns: &[T],
) -> Vec<BuiltQuery> {
    let index_name = vespertide_naming::build_index_name(table, columns, name);
    let mut idx = Index::create()
        .table(Alias::new(table))
        .name(&index_name)
        .to_owned();
    for col in columns {
        idx.col(Alias::new(col.as_ref()));
    }
    vec![BuiltQuery::CreateIndex(Box::new(idx))]
}
