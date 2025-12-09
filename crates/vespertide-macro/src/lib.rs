mod options;
mod runtime;

pub use options::MigrationOptions;
pub use runtime::{MigrationError, run_migrations};

/// Zero-runtime migration entry point.
#[macro_export]
macro_rules! vespertide_migration {
    ($pool:expr $(, $key:ident = $value:expr )* $(,)?) => {{
        async {
            let mut __version_table: &str = "vespertide_version";

            $(
                match stringify!($key) {
                    "version_table" => __version_table = $value,
                    _ => compile_error!("unsupported option for vespertide_migration!"),
                }
            )*

            $crate::run_migrations(
                $pool,
                $crate::MigrationOptions {
                    version_table: __version_table.to_string(),
                },
            )
            .await
        }
    }};
}
