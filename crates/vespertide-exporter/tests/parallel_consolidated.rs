use rayon::ThreadPoolBuilder;
use rstest::rstest;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, SimpleColumnType, TableDef};
use vespertide_exporter::Orm;

#[rstest]
#[case::seaorm(Orm::SeaOrm)]
#[case::sqlalchemy(Orm::SqlAlchemy)]
#[case::sqlmodel(Orm::SqlModel)]
#[case::jpa(Orm::Jpa)]
fn export_is_byte_identical_across_thread_counts(#[case] orm: Orm) {
    let schema = large_schema(100);

    let single_thread = ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build single-thread rayon pool")
        .install(|| render_schema(orm, &schema))
        .expect("single-thread export succeeds");

    let four_threads = ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .expect("build four-thread rayon pool")
        .install(|| render_schema(orm, &schema))
        .expect("four-thread export succeeds");

    assert_eq!(single_thread, four_threads);
}

fn render_schema(orm: Orm, schema: &[TableDef]) -> Result<String, String> {
    match orm {
        Orm::SeaOrm => vespertide_exporter::seaorm::export(schema),
        Orm::SqlAlchemy => vespertide_exporter::sqlalchemy::export(schema),
        Orm::SqlModel => vespertide_exporter::sqlmodel::render_entities(schema),
        Orm::Jpa => {
            vespertide_exporter::jpa::render_entities(schema).map(|entities| entities.join("\n"))
        }
        Orm::Prisma => vespertide_exporter::prisma::export(schema),
        Orm::Drizzle => vespertide_exporter::drizzle::export(schema),
    }
}

fn large_schema(count: usize) -> Vec<TableDef> {
    (0..count)
        .map(|idx| {
            TableDef {
                name: format!("parallel_table_{idx}").into(),
                description: Some(format!("Synthetic table {idx}")),
                columns: vec![
                    ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                        .primary_key(PrimaryKeySyntax::Bool(true)),
                    ColumnDef::new(
                        "name",
                        ColumnType::Complex(ComplexColumnType::Varchar { length: 191 }),
                        false,
                    ),
                    ColumnDef::new(
                        "created_at",
                        ColumnType::Simple(SimpleColumnType::Timestamptz),
                        false,
                    )
                    .default("NOW()".into()),
                    ColumnDef::new(
                        "external_id",
                        ColumnType::Simple(SimpleColumnType::Uuid),
                        true,
                    ),
                    ColumnDef::new("metadata", ColumnType::Simple(SimpleColumnType::Json), true),
                ],
                constraints: Vec::new(),
            }
            .normalize()
            .expect("generated table normalizes")
        })
        .collect()
}
