use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ColumnType, MigrationAction, SimpleColumnType, TableDef};
use vespertide_query::sql::helpers::quote_ident;
use vespertide_query::{DatabaseBackend, build_action_queries};

fn simple_type(ty: SimpleColumnType) -> ColumnType {
    ColumnType::Simple(ty)
}

fn build_table_n_columns(n_columns: usize) -> TableDef {
    let mut columns = vec![
        ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
            .primary_key(PrimaryKeySyntax::Bool(true)),
    ];
    columns.extend((1..n_columns).map(|i| {
        let ty = match i % 4 {
            0 => SimpleColumnType::Integer,
            1 => SimpleColumnType::Text,
            2 => SimpleColumnType::Boolean,
            _ => SimpleColumnType::Timestamptz,
        };
        ColumnDef::new(format!("column_{i}"), simple_type(ty), i % 5 == 0)
    }));

    TableDef {
        name: format!("wide_table_{n_columns}").into(),
        description: None,
        columns,
        constraints: vec![],
    }
}

fn bench_create_table_emit(c: &mut Criterion) {
    let mut group = c.benchmark_group("create_table_emit");
    for n_columns in [5, 50, 200] {
        let table = build_table_n_columns(n_columns);
        let action = MigrationAction::CreateTable {
            table: table.name.clone(),
            columns: table.columns.clone(),
            constraints: table.constraints.clone(),
        };
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            group.bench_with_input(
                BenchmarkId::new(format!("{backend:?}"), n_columns),
                &backend,
                |b, backend| {
                    b.iter(|| {
                        let queries = build_action_queries(
                            black_box(*backend),
                            black_box(&action),
                            black_box(&[]),
                        )
                        .expect("create-table SQL generation should succeed");
                        for query in &queries {
                            black_box(query.build(*backend));
                        }
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_representative_actions(c: &mut Criterion) {
    let mut group = c.benchmark_group("action_query_emit");
    let table = build_table_n_columns(25).normalize().expect("valid table");
    let schema = vec![table.clone()];
    let add_column = MigrationAction::AddColumn {
        table: table.name.clone(),
        column: Box::new(ColumnDef::new(
            "added_text",
            simple_type(SimpleColumnType::Text),
            true,
        )),
        fill_with: None,
    };
    let rename_column = MigrationAction::RenameColumn {
        table: table.name.clone(),
        from: "column_1".into(),
        to: "renamed_column_1".into(),
    };

    for (name, action) in [("add_column", add_column), ("rename_column", rename_column)] {
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            group.bench_with_input(
                BenchmarkId::new(name, format!("{backend:?}")),
                &backend,
                |b, backend| {
                    b.iter(|| {
                        let queries = build_action_queries(
                            black_box(*backend),
                            black_box(&action),
                            black_box(&schema),
                        )
                        .expect("representative action SQL generation should succeed");
                        for query in &queries {
                            black_box(query.build(*backend));
                        }
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_quote_ident(c: &mut Criterion) {
    let mut group = c.benchmark_group("quote_ident");
    for name in [
        "short",
        "medium_table_name",
        "a_very_long_table_name_indeed",
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &name, |b, name| {
            b.iter(|| quote_ident(black_box(name), black_box(DatabaseBackend::Postgres)));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_create_table_emit,
    bench_representative_actions,
    bench_quote_ident
);
criterion_main!(benches);
