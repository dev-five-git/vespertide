//! Fuzz SQL identifier quoting and raw-format SQL paths that consume
//! user-provided names. `quote_ident` must never panic and must produce
//! balanced outer delimiters.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vespertide_query::DatabaseBackend;
use vespertide_query::sql::helpers::quote_ident;

fuzz_target!(|data: &[u8]| {
    if let Ok(name) = std::str::from_utf8(data) {
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            let quoted = quote_ident(name, backend);

            assert!(
                !quoted.is_empty(),
                "quote_ident({name:?}, {backend:?}) produced empty"
            );

            let (open, close) = match backend {
                DatabaseBackend::MySql => ('`', '`'),
                DatabaseBackend::Postgres | DatabaseBackend::Sqlite => ('"', '"'),
            };
            assert!(quoted.starts_with(open), "no opening delim: {quoted:?}");
            assert!(quoted.ends_with(close), "no closing delim: {quoted:?}");
        }
    }
});
