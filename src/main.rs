use std::sync::Arc;
use std::time::Duration;

use ovalbot::calendar::CalendarService;
use ovalbot::mailer;
use ovalbot::notifications;
use ovalbot::state::AppState;
use ovalbot::{app, db};

const SYNC_INTERVAL_SECS: u64 = 20 * 60;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ovalbot=info,tower_http=info".into()),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://ovalbot.db".to_string());
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;

    let calendar = Arc::new(CalendarService::new());
    if let Err(e) = calendar.sync().await {
        tracing::warn!("initial calendar sync failed: {e}");
    }

    let state = AppState {
        pool,
        mailer: mailer::from_env(),
        calendar,
    };

    let sync_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));
        loop {
            interval.tick().await;
            match sync_state.calendar.sync().await {
                Ok(changes) => notifications::process(&sync_state, changes).await,
                Err(e) => tracing::warn!("calendar sync failed: {e}"),
            }
        }
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app(state)).await?;
    Ok(())
}
