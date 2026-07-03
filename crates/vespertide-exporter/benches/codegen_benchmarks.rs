use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, ReferenceAction, SimpleColumnType,
    TableDef, TableName,
};
use vespertide_exporter::{Orm, render_entity_with_schema};

const ALL_ORMS: [Orm; 4] = [Orm::SeaOrm, Orm::SqlAlchemy, Orm::SqlModel, Orm::Jpa];
const ENUM_ORMS: [Orm; 3] = [Orm::SeaOrm, Orm::SqlAlchemy, Orm::SqlModel];
const FK_COLUMNS_PER_TABLE: usize = 20;

fn simple_type(ty: SimpleColumnType) -> ColumnType {
    ColumnType::Simple(ty)
}

fn enum_type() -> ColumnType {
    ColumnType::Complex(ComplexColumnType::Enum {
        name: "record_status".to_string(),
        values: EnumValues::from(vec!["draft", "active", "archived"]),
    })
}

fn user_table() -> TableDef {
    TableDef {
        name: "user".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            ColumnDef::new("email", simple_type(SimpleColumnType::Text), false),
        ],
        constraints: vec![],
    }
    .normalize()
    .expect("valid user table")
}

fn build_table(n_columns: usize, with_fk: bool, with_enum: bool) -> TableDef {
    let mut columns = vec![
        ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
            .primary_key(PrimaryKeySyntax::Bool(true)),
    ];
    if with_fk {
        columns.push(
            ColumnDef::new("user_id", simple_type(SimpleColumnType::Integer), false).foreign_key(
                ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }),
            ),
        );
    }
    if with_enum {
        columns.push(ColumnDef::new("status", enum_type(), false));
    }
    while columns.len() < n_columns {
        let i = columns.len();
        let ty = match i % 5 {
            0 => simple_type(SimpleColumnType::Integer),
            1 => simple_type(SimpleColumnType::Text),
            2 => simple_type(SimpleColumnType::Boolean),
            3 => simple_type(SimpleColumnType::Timestamptz),
            _ => ColumnType::Complex(ComplexColumnType::Varchar { length: 191 }),
        };
        columns.push(ColumnDef::new(format!("field_{i}"), ty, i % 7 == 0));
    }

    TableDef {
        name: format!("entity_{n_columns}_{with_fk}_{with_enum}").into(),
        description: None,
        columns,
        constraints: vec![],
    }
    .normalize()
    .expect("valid generated benchmark table")
}

fn foreign_key_to(ref_table: impl Into<TableName>) -> ForeignKeySyntax {
    ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: ref_table.into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    })
}

fn build_fk_heavy_schema(n_tables: usize) -> Vec<TableDef> {
    assert!(n_tables > 1, "FK-heavy schema needs at least two tables");

    (0..n_tables)
        .map(|table_idx| {
            let mut table = build_table(1, false, false);
            table.name = format!("entity_{table_idx}").into();

            for fk_idx in 0..FK_COLUMNS_PER_TABLE {
                let target_idx = (table_idx + fk_idx + 1) % n_tables;
                let column_name = format!("entity_{target_idx}_id_{fk_idx}");

                table.columns.push(
                    ColumnDef::new(column_name, simple_type(SimpleColumnType::Integer), false)
                        .foreign_key(foreign_key_to(format!("entity_{target_idx}"))),
                );
            }

            table.normalize().expect("valid FK-heavy benchmark table")
        })
        .collect()
}

fn build_fk_chain_schema(depth: usize) -> Vec<TableDef> {
    (0..=depth)
        .map(|level| {
            let mut id_column = ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
                .primary_key(PrimaryKeySyntax::Bool(true));

            if level > 0 {
                id_column = id_column.foreign_key(foreign_key_to(format!("level_{}", level - 1)));
            }

            TableDef {
                name: format!("level_{level}").into(),
                description: None,
                columns: vec![id_column],
                constraints: vec![],
            }
            .normalize()
            .expect("valid FK-chain benchmark table")
        })
        .collect()
}

fn build_self_ref_schema() -> Vec<TableDef> {
    let employee = TableDef {
        name: "employee".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            ColumnDef::new("name", simple_type(SimpleColumnType::Text), false),
            ColumnDef::new("manager_id", simple_type(SimpleColumnType::Integer), true)
                .foreign_key(foreign_key_to("employee")),
        ],
        constraints: vec![],
    }
    .normalize()
    .expect("valid self-reference benchmark table");

    vec![employee]
}

fn build_table_with_large_enums(n_enums: usize, values_per_enum: usize) -> TableDef {
    let mut columns = vec![
        ColumnDef::new("id", simple_type(SimpleColumnType::Integer), false)
            .primary_key(PrimaryKeySyntax::Bool(true)),
    ];

    for enum_idx in 0..n_enums {
        let values = (0..values_per_enum)
            .map(|value_idx| format!("variant_{enum_idx}_{value_idx}"))
            .collect::<Vec<_>>();
        let ty = ColumnType::Complex(ComplexColumnType::Enum {
            name: format!("large_enum_{enum_idx}"),
            values: values.into(),
        });

        columns.push(ColumnDef::new(format!("enum_{enum_idx}"), ty, false));
    }

    TableDef {
        name: "large_enum_union".into(),
        description: None,
        columns,
        constraints: vec![],
    }
    .normalize()
    .expect("valid large-enum benchmark table")
}

fn bench_render_entity(c: &mut Criterion) {
    let mut group = c.benchmark_group("render_entity");
    let parent = user_table();

    for orm in ALL_ORMS {
        for n_columns in [10, 50, 200] {
            for (with_fk, with_enum) in [(false, false), (true, false), (false, true), (true, true)]
            {
                let table = build_table(n_columns, with_fk, with_enum);
                let schema = vec![parent.clone(), table.clone()];
                let case = format!("{orm:?}/cols={n_columns}/fk={with_fk}/enum={with_enum}");
                group.bench_with_input(BenchmarkId::from_parameter(case), &orm, |b, orm| {
                    b.iter(|| {
                        black_box(
                            render_entity_with_schema(
                                black_box(*orm),
                                black_box(&table),
                                black_box(&schema),
                            )
                            .expect("code generation should succeed"),
                        )
                    });
                });
            }
        }
    }
    group.finish();
}

fn bench_fk_heavy_schema(c: &mut Criterion) {
    let mut group = c.benchmark_group("fk_heavy_schema");
    let schema = build_fk_heavy_schema(50);
    let table = schema
        .iter()
        .find(|table| table.name == "entity_25")
        .expect("representative table exists")
        .clone();

    for orm in ALL_ORMS {
        let case = format!("{orm:?}/tables=50/fks={FK_COLUMNS_PER_TABLE}");
        group.bench_with_input(BenchmarkId::from_parameter(case), &orm, |b, orm| {
            b.iter(|| {
                black_box(
                    render_entity_with_schema(
                        black_box(*orm),
                        black_box(&table),
                        black_box(&schema),
                    )
                    .expect("code generation should succeed"),
                )
            });
        });
    }

    group.finish();
}

fn bench_fk_chain_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("fk_chain_depth");

    for depth in [1, 3, 5, 10] {
        let schema = build_fk_chain_schema(depth);
        let table = schema.last().expect("leaf table exists").clone();
        let case = format!("{:?}/depth={depth}", Orm::SeaOrm);

        group.bench_with_input(BenchmarkId::from_parameter(case), &depth, |b, _depth| {
            b.iter(|| {
                black_box(
                    render_entity_with_schema(
                        black_box(Orm::SeaOrm),
                        black_box(&table),
                        black_box(&schema),
                    )
                    .expect("code generation should succeed"),
                )
            });
        });
    }

    group.finish();
}

fn bench_self_reference_schema(c: &mut Criterion) {
    let mut group = c.benchmark_group("self_reference_schema");
    let schema = build_self_ref_schema();
    let table = schema.first().expect("self-reference table exists").clone();

    for orm in ALL_ORMS {
        let case = format!("{orm:?}/employee");
        group.bench_with_input(BenchmarkId::from_parameter(case), &orm, |b, orm| {
            b.iter(|| {
                black_box(
                    render_entity_with_schema(
                        black_box(*orm),
                        black_box(&table),
                        black_box(&schema),
                    )
                    .expect("code generation should succeed"),
                )
            });
        });
    }

    group.finish();
}

fn bench_large_enum_union(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_enum_union");
    let table = build_table_with_large_enums(10, 20);
    let schema = vec![table.clone()];

    for orm in ENUM_ORMS {
        let case = format!("{orm:?}/enums=10/values=20");
        group.bench_with_input(BenchmarkId::from_parameter(case), &orm, |b, orm| {
            b.iter(|| {
                black_box(
                    render_entity_with_schema(
                        black_box(*orm),
                        black_box(&table),
                        black_box(&schema),
                    )
                    .expect("code generation should succeed"),
                )
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_render_entity,
    bench_fk_heavy_schema,
    bench_fk_chain_depth,
    bench_self_reference_schema,
    bench_large_enum_union
);
criterion_main!(benches);
