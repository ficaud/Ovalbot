//! SQLite connection bootstrap shared by the server and the `create_admin` CLI.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::common::error::AppError;

/// Opens an SQLite connection pool described by `url` (e.g. `sqlite://app.db`).
///
/// Foreign keys are enforced (`users.sessions` / `ticket_requests` rely on
/// `ON DELETE CASCADE`) and the file is created when missing, so a fresh
/// deployment boots on an empty database.
///
/// # Arguments
/// * `url` — SQLite connect string (`sqlite://path`, `:memory:` also works).
///
/// # Returns
/// A pool capped at 5 connections (sufficient for SQLite single-writer).
///
/// # Errors
/// `AppError::Sqlx` when the URL is invalid or the database cannot be opened.
pub async fn connect(url: &str) -> Result<SqlitePool, AppError> {
    let options = SqliteConnectOptions::from_str(url)?
        .foreign_keys(true)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// Runs every pending SQL migration embedded at compile time by
/// `sqlx::migrate!` (the `migrations/` directory).
///
/// # Errors
/// `AppError::Migrate` when a migration fails to apply.
pub async fn migrate(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::migrate!().run(pool).await?;
    Ok(())
}
