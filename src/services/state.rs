use std::sync::Arc;

use sqlx::SqlitePool;

use crate::services::calendar::CalendarService;
use crate::services::mailer::Mailer;
use crate::services::telegram::{Telegram, TelegramConfig};
use crate::services::tickets::TicketManager;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub mailer: Arc<dyn Mailer>,
    pub telegram: Arc<dyn Telegram>,
    pub telegram_config: TelegramConfig,
    pub calendar: Arc<CalendarService>,
    pub tickets: Arc<TicketManager>,
}
