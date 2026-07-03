use proptest::prelude::*;
use rayon::ThreadPoolBuilder;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableDef};

proptest! {
    #![proptest_config(ProptestConfig { cases: 1000, max_shrink_iters: 5000, ..ProptestConfig::default() })]

    #[test]
    fn python_codegen_is_deterministic(table in arb_table_def()) {
        let Ok(table) = table.normalize() else {
            return Ok(());
        };

        for _ in 0..3 {
            let a1 = vespertide_exporter::sqlalchemy::render_entity(&table)
                .map_err(TestCaseError::fail)?;
            let a2 = vespertide_exporter::sqlalchemy::render_entity(&table)
                .map_err(TestCaseError::fail)?;
            prop_assert_eq!(a1, a2, "sqlalchemy non-deterministic");

            let m1 = vespertide_exporter::sqlmodel::render_entity(&table)
                .map_err(TestCaseError::fail)?;
            let m2 = vespertide_exporter::sqlmodel::render_entity(&table)
                .map_err(TestCaseError::fail)?;
            prop_assert_eq!(m1, m2, "sqlmodel non-deterministic");

            let s1 = vespertide_exporter::seaorm::render_entity(&table);
            let s2 = vespertide_exporter::seaorm::render_entity(&table);
            prop_assert_eq!(s1, s2, "seaorm non-deterministic");

            let j1 = vespertide_exporter::jpa::render_entity(&table)
                .map_err(TestCaseError::fail)?;
            let j2 = vespertide_exporter::jpa::render_entity(&table)
                .map_err(TestCaseError::fail)?;
            prop_assert_eq!(j1, j2, "jpa non-deterministic");
        }
    }
}

#[test]
fn sqlmodel_parallel_render_entities_is_thread_count_deterministic() {
    let schema = wide_schema(200);
    let one_thread = ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build one-thread rayon pool");
    let eight_threads = ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .expect("build eight-thread rayon pool");

    for _ in 0..100 {
        let single = one_thread
            .install(|| vespertide_exporter::sqlmodel::render_entities(&schema))
            .expect("render sqlmodel entities with one rayon thread");
        let parallel = eight_threads
            .install(|| vespertide_exporter::sqlmodel::render_entities(&schema))
            .expect("render sqlmodel entities with eight rayon threads");

        assert_eq!(single, parallel);
    }
}

fn wide_schema(table_count: usize) -> Vec<TableDef> {
    (0..table_count)
        .map(|index| TableDef {
            name: format!("table_{index:03}").into(),
            description: Some(format!("Table {index}")),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "name".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: Some(format!("Name for table {index}")),
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: Vec::new(),
        })
        .collect()
}

fn arb_table_def() -> impl Strategy<Value = TableDef> {
    (
        arb_safe_ident(),
        prop::collection::vec((arb_safe_ident(), arb_column_type(), any::<bool>()), 0..=8)
            .prop_filter("unique column names", |columns| {
                let mut names = std::collections::BTreeSet::new();
                columns.iter().all(|(name, _, _)| names.insert(name))
            }),
    )
        .prop_map(|(name, columns)| TableDef {
            name: name.into(),
            description: None,
            columns: columns
                .into_iter()
                .map(|(name, r#type, nullable)| ColumnDef {
                    name: name.into(),
                    r#type,
                    nullable,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                })
                .collect(),
            constraints: Vec::new(),
        })
}

fn arb_column_type() -> impl Strategy<Value = ColumnType> {
    prop_oneof![
        Just(SimpleColumnType::SmallInt),
        Just(SimpleColumnType::Integer),
        Just(SimpleColumnType::BigInt),
        Just(SimpleColumnType::Real),
        Just(SimpleColumnType::DoublePrecision),
        Just(SimpleColumnType::Text),
        Just(SimpleColumnType::Boolean),
        Just(SimpleColumnType::Date),
        Just(SimpleColumnType::Time),
        Just(SimpleColumnType::Timestamp),
        Just(SimpleColumnType::Timestamptz),
        Just(SimpleColumnType::Interval),
        Just(SimpleColumnType::Bytea),
        Just(SimpleColumnType::Uuid),
        Just(SimpleColumnType::Json),
        Just(SimpleColumnType::Inet),
        Just(SimpleColumnType::Cidr),
        Just(SimpleColumnType::Macaddr),
        Just(SimpleColumnType::Xml),
    ]
    .prop_map(ColumnType::Simple)
}

fn arb_safe_ident() -> impl Strategy<Value = String> {
    (
        prop::char::range('a', 'z'),
        prop::collection::vec(
            prop_oneof![
                prop::char::range('a', 'z'),
                prop::char::range('0', '9'),
                Just('_'),
            ],
            0..=20,
        ),
    )
        .prop_map(|(first, rest)| {
            let mut ident = String::with_capacity(rest.len() + 1);
            ident.push(first);
            ident.extend(rest);
            ident
        })
}
