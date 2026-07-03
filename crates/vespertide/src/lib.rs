//! # Vespertide
//!
//! Declarative database schema management for Rust. Define schemas in JSON,
//! generate migration plans, emit SQL for PostgreSQL/MySQL/SQLite, and export
//! ORM models for SeaORM/SQLAlchemy/SQLModel/JPA.
//!
//! This is the facade crate; runtime migrations use [`vespertide_migration!`].
//! Advanced users may depend on `vespertide-core` directly for typed data structures.
//!
//! ## Quick Start
//!
//! Add to `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! vespertide = "0.2"
//! sea-orm = { version = "2", features = ["sqlx-postgres", "runtime-tokio-native-tls"] }
//! ```
//!
//! Run migrations at application startup:
//!
//! ```rust,ignore
//! use sea_orm::Database;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let db = Database::connect("postgres://user:pass@localhost/mydb").await?;
//!     vespertide::vespertide_migration!(db).await?;
//!     Ok(())
//! }
//! ```
//!
//! Customise the version-tracking table name:
//!
//! ```rust
//! use vespertide::MigrationOptions;
//!
//! // MigrationOptions::new() is the simplest constructor.
//! let opts = MigrationOptions::new("app_schema_versions");
//! assert_eq!(opts.version_table, "app_schema_versions");
//!
//! // Default gives the standard table name.
//! let default_opts = MigrationOptions::default();
//! assert_eq!(default_opts.version_table, "vespertide_migrations");
//! ```

pub mod runtime;

// Re-export macro for convenient usage
#[doc(inline)]
pub use vespertide_macro::vespertide_migration;

// Re-export other commonly used items
pub use vespertide_core::{MigrationError, MigrationOptions};
