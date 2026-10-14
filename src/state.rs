use std::sync::Arc;

use sqlx::SqlitePool;

use crate::calendar::CalendarService;
use crate::mailer::Mailer;
use crate::telegram::{Telegram, TelegramConfig};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub mailer: Arc<dyn Mailer>,
    pub telegram: Arc<dyn Telegram>,
    pub telegram_config: TelegramConfig,
    pub calendar: Arc<CalendarService>,
}
