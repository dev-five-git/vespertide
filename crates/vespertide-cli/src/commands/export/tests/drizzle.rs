use super::*;
use insta::{assert_snapshot, with_settings};
use vespertide_exporter::drizzle::DrizzleDialect;

/// One export writes one file per dialect — Drizzle's table constructors fork
/// at the `import` line, so there is no backend-neutral single file. Each case
/// pins the file that dialect actually leaves on disk, end to end from the
/// model JSON.
#[rstest]
#[case::pg(DrizzleDialect::Pg)]
#[case::mysql(DrizzleDialect::Mysql)]
#[case::sqlite(DrizzleDialect::Sqlite)]
#[serial]
#[tokio::test]
async fn export_drizzle_writes_one_file_per_dialect(#[case] dialect: DrizzleDialect) {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();
    write_model(Path::new("models/events.json"), &sample_table("events"));

    cmd_export(Orm::Drizzle, None).await.unwrap();

    let out = PathBuf::from("src/models").join(format!("models.{}.ts", dialect.file_suffix()));
    let written = std_fs::read_to_string(out).unwrap();
    with_settings!({ snapshot_suffix => dialect.file_suffix() }, {
        assert_snapshot!(written);
    });
}

#[test]
fn build_output_path_drizzle_uses_ts_extension() {
    use std::path::Path;
    let root = Path::new("src/models");
    let out = build_output_path(root, Path::new("user.json"), Orm::Drizzle);
    assert_eq!(out, Path::new("src/models/user.ts"));
}

/// The Drizzle path deliberately skips the `.ts` extension sweep the other
/// ORMs run: the export root doubles as a source directory, so the user's own
/// files — and the directories holding them, which the sweep prunes once
/// emptied — must survive an export, while the dialect's own stale output is
/// replaced. One case per dialect, since each writes a different file; what
/// lands in it is pinned by the snapshots above, so here only survival and
/// replacement are asserted.
#[rstest]
#[case::pg(DrizzleDialect::Pg)]
#[case::mysql(DrizzleDialect::Mysql)]
#[case::sqlite(DrizzleDialect::Sqlite)]
#[serial]
#[tokio::test]
async fn export_drizzle_preserves_user_ts_files(#[case] dialect: DrizzleDialect) {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();
    write_model(Path::new("models/events.json"), &sample_table("events"));

    let root = PathBuf::from("src/models");
    let generated = root.join(format!("models.{}.ts", dialect.file_suffix()));
    std_fs::create_dir_all(root.join("helpers")).unwrap();
    std_fs::write(root.join("index.ts"), "export {};").unwrap();
    std_fs::write(root.join("helpers/util.ts"), "export {};").unwrap();
    std_fs::write(&generated, "stale").unwrap();

    cmd_export(Orm::Drizzle, None).await.unwrap();

    assert_eq!(
        std_fs::read_to_string(root.join("index.ts")).unwrap(),
        "export {};"
    );
    assert_eq!(
        std_fs::read_to_string(root.join("helpers/util.ts")).unwrap(),
        "export {};"
    );
    assert_ne!(std_fs::read_to_string(&generated).unwrap(), "stale");
}
