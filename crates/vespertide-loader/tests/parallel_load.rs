use std::fs;
use std::path::{Path, PathBuf};

use rayon::{ThreadPool, ThreadPoolBuilder};
use serial_test::serial;
use tempfile::TempDir;
use vespertide_config::VespertideConfig;
use vespertide_loader::{
    load_migrations, load_migrations_from_dir, load_models, load_models_from_dir,
};

const FIXTURE_COUNT: usize = 30;

struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    fn new(dir: &Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

fn create_project() -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let config = serde_json::to_string_pretty(&VespertideConfig::default()).unwrap();
    fs::write(temp_dir.path().join("vespertide.json"), config).unwrap();
    temp_dir
}

fn write_model(path: &Path, index: usize) {
    let column_count = index % 20 + 1;
    let mut columns = vec![
        r#"{"name":"id","type":"integer","nullable":false,"primary_key":{"auto_increment":true}}"#
            .to_string(),
    ];
    columns.extend((0..column_count).map(|col| {
        format!(r#"{{"name":"col_{col:02}","type":"integer","nullable":false,"default":{col}}}"#)
    }));

    let content = format!(
        r#"{{"$schema":"https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json","name":"table_{index:03}","columns":[{}]}}"#,
        columns.join(",")
    );
    fs::write(path, content).unwrap();
}

fn write_models(project_root: &Path) {
    let models_dir = project_root.join("models");
    fs::create_dir_all(&models_dir).unwrap();
    for index in (0..FIXTURE_COUNT).rev() {
        write_model(&models_dir.join(format!("table_{index:03}.json")), index);
    }
}

fn write_migrations(project_root: &Path) {
    let migrations_dir = project_root.join("migrations");
    fs::create_dir_all(&migrations_dir).unwrap();
    for index in (1..=FIXTURE_COUNT).rev() {
        fs::write(
            migrations_dir.join(format!("{index:04}_migration.json")),
            format!(r#"{{"version":{index},"actions":[]}}"#),
        )
        .unwrap();
    }
}

fn thread_pool(thread_count: usize) -> ThreadPool {
    ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .expect("rayon thread pool should build")
}

#[test]
#[serial]
fn load_models_parallel_matches_single_threaded_result() {
    let temp_dir = create_project();
    write_models(temp_dir.path());
    let _cwd = CwdGuard::new(temp_dir.path());
    let one_thread = thread_pool(1);
    let eight_threads = thread_pool(8);

    let single = one_thread.install(|| load_models(&VespertideConfig::default()).unwrap());
    let parallel = eight_threads.install(|| load_models(&VespertideConfig::default()).unwrap());

    assert_eq!(single, parallel);
}

#[test]
#[serial]
fn load_models_from_dir_parallel_matches_single_threaded_result() {
    let temp_dir = create_project();
    write_models(temp_dir.path());
    let root = temp_dir.path().to_path_buf();
    let one_thread = thread_pool(1);
    let eight_threads = thread_pool(8);

    let single = one_thread.install(|| load_models_from_dir(Some(root.clone())).unwrap());
    let parallel = eight_threads.install(|| load_models_from_dir(Some(root)).unwrap());

    assert_eq!(single, parallel);
}

#[test]
#[serial]
fn load_models_reports_earliest_invalid_file_by_path() {
    let temp_dir = create_project();
    write_models(temp_dir.path());
    fs::write(temp_dir.path().join("models/table_010.json"), "{").unwrap();
    let _cwd = CwdGuard::new(temp_dir.path());
    let one_thread = thread_pool(1);
    let eight_threads = thread_pool(8);

    let single = one_thread.install(|| load_models(&VespertideConfig::default()).unwrap_err());
    let parallel = eight_threads.install(|| load_models(&VespertideConfig::default()).unwrap_err());

    assert_eq!(single.to_string(), parallel.to_string());
    assert!(single.to_string().contains("table_010.json"));
}

#[test]
#[serial]
fn load_migrations_parallel_matches_single_threaded_result() {
    let temp_dir = create_project();
    write_migrations(temp_dir.path());
    let _cwd = CwdGuard::new(temp_dir.path());
    let one_thread = thread_pool(1);
    let eight_threads = thread_pool(8);

    let single = one_thread.install(|| load_migrations(&VespertideConfig::default()).unwrap());
    let parallel = eight_threads.install(|| load_migrations(&VespertideConfig::default()).unwrap());

    assert_eq!(single, parallel);
}

#[test]
#[serial]
fn load_migrations_from_dir_parallel_matches_single_threaded_result() {
    let temp_dir = create_project();
    write_migrations(temp_dir.path());
    let root = temp_dir.path().to_path_buf();
    let one_thread = thread_pool(1);
    let eight_threads = thread_pool(8);

    let single = one_thread.install(|| load_migrations_from_dir(Some(root.clone())).unwrap());
    let parallel = eight_threads.install(|| load_migrations_from_dir(Some(root)).unwrap());

    assert_eq!(single, parallel);
}

#[test]
#[serial]
fn load_migrations_reports_earliest_invalid_file_by_path() {
    let temp_dir = create_project();
    write_migrations(temp_dir.path());
    fs::write(temp_dir.path().join("migrations/0010_migration.json"), "{").unwrap();
    let _cwd = CwdGuard::new(temp_dir.path());
    let one_thread = thread_pool(1);
    let eight_threads = thread_pool(8);

    let single = one_thread.install(|| load_migrations(&VespertideConfig::default()).unwrap_err());
    let parallel =
        eight_threads.install(|| load_migrations(&VespertideConfig::default()).unwrap_err());

    assert_eq!(single.to_string(), parallel.to_string());
    assert!(single.to_string().contains("0010_migration.json"));
}
