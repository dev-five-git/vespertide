use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ReferenceAction, SimpleColumnType, StrOrBoolOrArray, TableDef,
};

fn simple_type(ty: SimpleColumnType) -> ColumnType {
    ColumnType::Simple(ty)
}

fn build_table(n_columns: usize, with_inline_constraints: bool) -> TableDef {
    let mut columns = Vec::with_capacity(n_columns.max(1));
    columns.push(
        ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
            .primary_key(PrimaryKeySyntax::Bool(true)),
    );

    for i in 1..n_columns {
        let mut column = ColumnDef::new(
            format!("column_{i}"),
            if i % 3 == 0 {
                simple_type(SimpleColumnType::Integer)
            } else {
                simple_type(SimpleColumnType::Text)
            },
            i % 7 == 0,
        );

        if with_inline_constraints {
            if i % 5 == 0 {
                column = column.index(StrOrBoolOrArray::Bool(true));
            }
            if i % 11 == 0 {
                column = column.unique(StrOrBoolOrArray::Str(format!("uq_norm__column_{i}")));
            }
            if i % 17 == 0 {
                column = column.foreign_key(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "parent".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }));
            }
        }

        columns.push(column);
    }

    TableDef {
        name: format!("normalize_{n_columns}_{with_inline_constraints}").into(),
        description: None,
        columns,
        constraints: vec![],
    }
}

fn bench_normalize(c: &mut Criterion) {
    let mut group = c.benchmark_group("table_normalize");
    for n_columns in [10, 100, 500] {
        for with_inline_constraints in [false, true] {
            let table = build_table(n_columns, with_inline_constraints);
            group.bench_with_input(
                BenchmarkId::new(
                    if with_inline_constraints {
                        "with_inline_constraints"
                    } else {
                        "without_inline_constraints"
                    },
                    n_columns,
                ),
                &table,
                |b, table| b.iter(|| black_box(table).normalize()),
            );
        }
    }
    group.finish();
}

criterion_group!(benches, bench_normalize);
criterion_main!(benches);
