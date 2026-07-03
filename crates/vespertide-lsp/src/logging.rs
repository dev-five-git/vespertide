//! Logging setup for the LSP binary.
//!
//! Zed (and several other editors) pipe LSP stderr into a shared log buffer
//! where messages from multiple servers interleave, making vespertide-lsp
//! activity hard to spot. To make troubleshooting straightforward we also
//! tee every event to a dedicated log file.
//!
//! Resolution order for the log file path:
//! 1. `$VESPERTIDE_LSP_LOG` if set.
//! 2. `$TMPDIR` / `%TEMP%` / `/tmp` + `vespertide-lsp.log` otherwise.
//!
//! Set `$VESPERTIDE_LSP_LOG=` (empty) to disable file logging entirely.
//! Log level is controlled by `$RUST_LOG` (defaults to `info`).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// Initialize tracing for the LSP binary. Returns the log file path that
/// was opened (if any) so the caller can print it on startup.
#[cfg(not(tarpaulin_include))]
// reason: global logging/telemetry init, non-deterministic under cargo test
pub fn init() -> Option<PathBuf> {
    let path = resolved_log_path();
    let file = path
        .as_ref()
        .and_then(|p| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .map_err(|e| {
                    let _ = writeln!(
                        std::io::stderr(),
                        "[vespertide-lsp] failed to open log file {}: {e}",
                        p.display()
                    );
                    e
                })
                .ok()
        })
        .map(|f| Arc::new(Mutex::new(f)));

    let writer = TeeWriter { file };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .init();

    path
}

fn resolved_log_path() -> Option<PathBuf> {
    match std::env::var_os("VESPERTIDE_LSP_LOG") {
        // Explicit empty string disables file logging.
        Some(s) if s.is_empty() => None,
        Some(s) => Some(PathBuf::from(s)),
        None => Some(std::env::temp_dir().join("vespertide-lsp.log")),
    }
}

/// Writer that fans events out to stderr (always) and an optional log file.
///
/// Stderr is kept because some workflows still rely on it (e.g. running the
/// server under `strace`/`dtrace`) and `tracing` adds negligible overhead.
struct TeeWriter {
    file: Option<Arc<Mutex<File>>>,
}

impl<'a> MakeWriter<'a> for TeeWriter {
    type Writer = TeeWriterHandle;

    fn make_writer(&'a self) -> Self::Writer {
        TeeWriterHandle {
            file: self.file.clone(),
        }
    }
}

struct TeeWriterHandle {
    file: Option<Arc<Mutex<File>>>,
}

impl Write for TeeWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Stderr is best-effort — never fail an LSP event because the
        // editor closed its log pipe. The file write is the source of truth.
        let _ = io::stderr().write_all(buf);
        if let Some(file) = &self.file
            && let Ok(mut guard) = file.lock()
        {
            guard.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        if let Some(file) = &self.file
            && let Ok(mut guard) = file.lock()
        {
            guard.flush()?;
        }
        Ok(())
    }
}
