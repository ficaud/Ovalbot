//! Shared dependency container handed to every Axum handler and background
//! service — the single place wiring implementations to traits.

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::services::calendar::CalendarService;
use crate::services::channels::mailer::Mailer;
use crate::services::channels::telegram::{Telegram, TelegramConfig};

/// All dependencies of the app, cheap-to-clone (pools and `Arc`s only).
///
/// Concrete transports are injected behind traits ([`Mailer`], [`Telegram`])
/// so tests swap in fakes; the config value lives alongside the client because
/// handlers need the bot username/deep link without an HTTP client.
#[derive(Clone)]
pub struct AppState {
    /// Shared SQLite connection pool (also used by background services).
    pub pool: SqlitePool,
    /// Welcome-email and match-alert transport.
    pub mailer: Arc<dyn Mailer>,
    /// Telegram transport (a no-op when `TELEGRAM_ENABLED` is not `true`).
    pub telegram: Arc<dyn Telegram>,
    /// Static Telegram settings: bot username for the linking deep link.
    pub telegram_config: TelegramConfig,
    /// Shared in-memory calendar cache used by reads and the bot scanner.
    pub calendar: Arc<CalendarService>,
}
