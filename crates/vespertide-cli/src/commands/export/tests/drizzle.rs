use super::*;

/// One export writes one file per dialect — Drizzle's table constructors fork
/// at the `import` line, so there is no backend-neutral single file.
#[tokio::test]
#[serial]
async fn export_drizzle_writes_one_file_per_dialect() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();
    write_model(Path::new("models/events.json"), &sample_table("events"));

    cmd_export(Orm::Drizzle, None).await.unwrap();

    let root = PathBuf::from("src/models");
    let pg = std_fs::read_to_string(root.join("models.pg.ts")).unwrap();
    let mysql = std_fs::read_to_string(root.join("models.mysql.ts")).unwrap();
    let sqlite = std_fs::read_to_string(root.join("models.sqlite.ts")).unwrap();

    assert!(pg.contains("pgTable(\"events\""));
    assert!(pg.contains("from \"drizzle-orm/pg-core\""));
    assert!(mysql.contains("mysqlTable(\"events\""));
    assert!(mysql.contains("from \"drizzle-orm/mysql-core\""));
    assert!(sqlite.contains("sqliteTable(\"events\""));
    assert!(sqlite.contains("from \"drizzle-orm/sqlite-core\""));
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
/// files must survive an export, and the three fixed outputs are simply
/// overwritten in place.
#[tokio::test]
#[serial]
async fn export_drizzle_preserves_user_ts_files() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();
    write_model(Path::new("models/events.json"), &sample_table("events"));

    let root = PathBuf::from("src/models");
    std_fs::create_dir_all(root.join("helpers")).unwrap();
    std_fs::write(root.join("index.ts"), "export {};").unwrap();
    std_fs::write(root.join("helpers/util.ts"), "export {};").unwrap();
    std_fs::write(root.join("models.pg.ts"), "stale").unwrap();

    cmd_export(Orm::Drizzle, None).await.unwrap();

    assert_eq!(
        std_fs::read_to_string(root.join("index.ts")).unwrap(),
        "export {};"
    );
    assert_eq!(
        std_fs::read_to_string(root.join("helpers/util.ts")).unwrap(),
        "export {};"
    );
    let pg = std_fs::read_to_string(root.join("models.pg.ts")).unwrap();
    assert!(pg.contains("pgTable(\"events\""));
}
