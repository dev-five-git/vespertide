use super::*;
use insta::assert_snapshot;

/// `app_label` is the one `django` config setting, and it only reaches the
/// generated `Meta` class through `DjangoExporterWithConfig`. Exporting with it
/// set is what proves the CLI takes that path.
#[tokio::test]
#[serial]
async fn export_django_writes_the_configured_app_label_into_meta() {
    let tmp = tempdir().unwrap();
    let _guard = CwdGuard::new(&tmp.path().to_path_buf());
    let mut cfg = serde_json::to_value(VespertideConfig::default()).unwrap();
    cfg["django"] = serde_json::json!({ "appLabel": "storefront" });
    std_fs::write(
        "vespertide.json",
        serde_json::to_string_pretty(&cfg).unwrap(),
    )
    .unwrap();
    write_model(Path::new("models/gadgets.json"), &sample_table("gadgets"));

    cmd_export(Orm::Django, None).await.unwrap();

    let written = std_fs::read_to_string(PathBuf::from("src/models/models.py")).unwrap();
    assert_snapshot!(written);
}
