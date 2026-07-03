//! Integration-test-only crate for sea-orm runtime tests that require the
//! `sqlx-sqlite` backend. Kept out of the main workspace to avoid the
//! `links = "sqlite3"` collision with vespertide-query's rusqlite dev-dep.
//!
//! See `Cargo.toml` and `tests/` for details.
