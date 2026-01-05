use anyhow::Result;
use sea_orm::{ConnectOptions, Database};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Hello, world!");

    let mut opt = ConnectOptions::new("postgres://postgres:password@localhost:5432/postgres");
    // Configure SQLite connection
    // let mut opt = ConnectOptions::new("sqlite://./local.db");
    opt.max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .max_lifetime(Duration::from_secs(8))
        .sqlx_logging(false); // Disable SQLx logging

    // Connect to the database
    let db = Database::connect(opt).await?;

    println!("Successfully connected to SQLite database!");

    vespertide::vespertide_migration!(db, version_table = "vespertide_version").await?;

    Ok(())
}
