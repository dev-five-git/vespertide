//! `vespertide-lsp` binary entry point.
//!
//! Wires the [`Backend`] into a tower-lsp-server stdio transport so editors
//! (VS Code, Neovim, etc.) can spawn the language server and communicate
//! over stdin/stdout. Tracing events go to stderr AND (by default) a
//! dedicated log file so Zed's interleaved log buffer is not the only
//! place to look — see [`vespertide_lsp::logging`] for the resolution
//! order. stdout is reserved for LSP framed JSON-RPC.

use tower_lsp_server::{LspService, Server};
use vespertide_lsp::Backend;
use vespertide_lsp::logging;

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() {
    // reason: binary entrypoint - stdio LSP server, not unit-testable
    let log_path = logging::init();

    let pid = std::process::id();
    let version = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe().ok().map_or_else(
        || "<unknown>".to_string(),
        |p| p.to_string_lossy().into_owned(),
    );
    let build_time = env!("CARGO_PKG_VERSION");
    if let Some(path) = log_path.as_ref() {
        tracing::info!(
            target: "vespertide_lsp",
            pid,
            version,
            build = build_time,
            exe = %exe,
            log_file = %path.display(),
            "vespertide-lsp starting"
        );
    } else {
        tracing::info!(
            target: "vespertide_lsp",
            pid,
            version,
            build = build_time,
            exe = %exe,
            "vespertide-lsp starting (file logging disabled)"
        );
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    tracing::info!(target: "vespertide_lsp", "vespertide-lsp shutting down");
}
