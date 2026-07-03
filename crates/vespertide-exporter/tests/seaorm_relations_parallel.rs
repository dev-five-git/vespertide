use rayon::ThreadPoolBuilder;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};

#[test]
fn seaorm_fk_relation_resolution_matches_sequential_and_parallel_schema_sizes() {
    let parallel_schema = hundred_fk_table_fixture();
    let sequential_schema = parallel_schema[..5].to_vec();

    let sequential = vespertide_exporter::seaorm::render_entity_with_schema(
        &sequential_schema[0],
        &sequential_schema,
    );
    let parallel = vespertide_exporter::seaorm::render_entity_with_schema(
        &parallel_schema[0],
        &parallel_schema,
    );

    assert_eq!(sequential, parallel);
}

#[test]
fn seaorm_fk_heavy_export_byte_identical_across_thread_counts() {
    let schema = hundred_fk_table_fixture();

    let single_thread = ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build single-thread rayon pool")
        .install(|| vespertide_exporter::seaorm::export(&schema))
        .expect("single-thread SeaORM export");

    let four_threads = ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .expect("build four-thread rayon pool")
        .install(|| vespertide_exporter::seaorm::export(&schema))
        .expect("four-thread SeaORM export");

    assert_eq!(single_thread, four_threads);
}

fn hundred_fk_table_fixture() -> Vec<TableDef> {
    (0..100)
        .map(|idx| {
            let refs = relation_targets_for(idx);
            fk_table(idx, &refs)
        })
        .collect()
}

fn relation_targets_for(idx: usize) -> [usize; 3] {
    match idx {
        1..=3 => [4, 2, 3],
        _ => [1, 2, 3],
    }
}

fn fk_table(idx: usize, refs: &[usize; 3]) -> TableDef {
    let mut columns = vec![int_col("id")];
    let mut constraints = vec![TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: vec!["id".into()],
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }];

    for (fk_idx, target) in refs.iter().enumerate() {
        let column = if (1..=3).contains(&idx) && fk_idx == 0 {
            "id".to_string()
        } else {
            format!("table_{target}_id")
        };

        if column != "id" && !columns.iter().any(|existing| existing.name == column) {
            columns.push(int_col(&column));
        }

        constraints.push(TableConstraint::ForeignKey {
            name: None,
            columns: vec![column.into()],
            ref_table: format!("table_{target}").into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        });
    }

    TableDef {
        name: format!("table_{idx}").into(),
        description: None,
        columns,
        constraints,
    }
}

fn int_col(name: &str) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}
