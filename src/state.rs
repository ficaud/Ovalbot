use std::sync::Arc;

use sqlx::SqlitePool;

use crate::mailer::Mailer;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub mailer: Arc<dyn Mailer>,
}
