use std::fs;
use std::future::Future;
use std::str::FromStr;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::{Service, ServiceExt};
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::ls_types::{DidOpenTextDocumentParams, Position, Range, Uri};
use tower_lsp_server::{ClientSocket, LanguageServer, LspService};

use super::super::Backend;

pub(super) const USER_URI: &str = "file:///workspace/user.json";
pub(super) const POST_URI: &str = "file:///workspace/post.json";
pub(super) const UNKNOWN_URI: &str = "file:///workspace/missing.json";
pub(super) const TEXT_URI: &str = "file:///workspace/plain.txt";

pub(super) const USER_MODEL: &str = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true,"unique":true,"index":true},{"name":"email","type":"text","nullable":false,"unique":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age BETWEEN 100 AND 0"}]}"#;

pub(super) const POST_MODEL: &str = r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;

pub(super) const USER_YAML: &str = "name: account\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\n    primary_key: true\n  - name: email\n    type: text\n";

pub(super) const MULTILINE_MODEL: &str = r#"{
  "name": "user",
  "columns": [
    {
      "name": "id",
      "type": "integer",
      "nullable": false,
      "primary_key": true
    },
    {
      "name": "email",
      "type": "text",
      "nullable": false
    }
  ]
}
"#;

pub(super) struct WorkspaceFixture {
    pub(super) _tmp: TempDir,
    pub(super) root_uri: Uri,
    pub(super) user_uri: Uri,
    pub(super) post_uri: Uri,
    pub(super) outside_uri: Uri,
}

pub(super) fn make_service() -> (LspService<Backend>, ClientSocket) {
    enable_tracing();
    LspService::new(Backend::new)
}

pub(super) fn block_on_with_trace(future: impl Future<Output = ()>) {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future);
    });
}

fn enable_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();
}

pub(super) fn uri(text: &str) -> Uri {
    Uri::from_str(text).unwrap()
}

pub(super) fn params<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

pub(super) fn position(source: &str, needle: &str, advance: usize) -> Position {
    let byte = source.find(needle).unwrap() + advance;
    position_at_byte(source, byte)
}

pub(super) fn position_at_byte(source: &str, byte: usize) -> Position {
    let prefix = &source[..byte];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let character = prefix
        .rsplit('\n')
        .next()
        .map_or(0, |line| line.encode_utf16().count());
    Position {
        line: u32::try_from(line).unwrap(),
        character: u32::try_from(character).unwrap(),
    }
}

pub(super) fn full_range(source: &str) -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: position_at_byte(source, source.len()),
    }
}

pub(super) async fn open_doc(backend: &Backend, uri: &Uri, language_id: &str, text: &str) {
    backend
        .did_open(did_open_params(uri, language_id, 1, text))
        .await;
}

pub(super) fn did_open_params(
    uri: &Uri,
    language_id: &str,
    version: i32,
    text: &str,
) -> DidOpenTextDocumentParams {
    params(json!({
        "textDocument": {
            "uri": uri.as_str(),
            "languageId": language_id,
            "version": version,
            "text": text,
        }
    }))
}

pub(super) async fn initialize_service(service: &mut LspService<Backend>, init_params: Value) {
    let request = Request::build("initialize")
        .params(init_params)
        .id(1)
        .finish();
    let response = service.ready().await.unwrap().call(request).await.unwrap();
    assert!(response.is_some(), "initialize must return a response");
}

pub(super) async fn notify(service: &mut LspService<Backend>, method: &'static str, params: Value) {
    let request = Request::build(method).params(params).finish();
    let response = service.ready().await.unwrap().call(request).await.unwrap();
    assert!(response.is_none(), "{method} is a notification");
}

pub(super) fn workspace_fixture() -> WorkspaceFixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let models_dir = root.join("models");
    let migrations_dir = root.join("migrations");
    fs::create_dir_all(&models_dir).unwrap();
    fs::create_dir_all(&migrations_dir).unwrap();
    fs::write(root.join("vespertide.json"), workspace_config()).unwrap();

    let user_path = models_dir.join("user.json");
    let post_path = models_dir.join("post.json");
    let outside_path = root.join("README.md");
    fs::write(&user_path, USER_MODEL).unwrap();
    fs::write(&post_path, POST_MODEL).unwrap();
    fs::write(&outside_path, "not a model").unwrap();

    WorkspaceFixture {
        root_uri: Backend::path_to_uri(root).unwrap(),
        user_uri: Backend::path_to_uri(&user_path).unwrap(),
        post_uri: Backend::path_to_uri(&post_path).unwrap(),
        outside_uri: Backend::path_to_uri(&outside_path).unwrap(),
        _tmp: tmp,
    }
}

pub(super) fn workspace_config() -> &'static str {
    r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#
}
