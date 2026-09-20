use super::*;
use insta::assert_snapshot;

/// Go requires the `package` clause to match the directory the file lives in,
/// so the effective package name comes from the real write target rather than
/// the config's static default. Exporting into a non-default directory is what
/// tells the two apart.
#[tokio::test]
#[serial]
async fn export_gorm_takes_its_package_name_from_the_export_directory() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    write_config();
    write_model(Path::new("models/widgets.json"), &sample_table("widgets"));

    cmd_export(Orm::Gorm, Some(PathBuf::from("generated/store")))
        .await
        .unwrap();

    let written = std_fs::read_to_string(PathBuf::from("generated/store/models.go")).unwrap();
    assert_snapshot!(written);
}
