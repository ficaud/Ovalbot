use calendar::Change;
use sqlx::SqlitePool;

use crate::common::error::AppError;
use crate::services::mailer::MatchAlert;
use crate::services::state::AppState;

pub async fn subscribe(pool: &SqlitePool, user_id: i64, match_id: i64) -> Result<(), AppError> {
    let now = crate::common::time::now();
    sqlx::query(
        "INSERT INTO match_notifications (user_id, match_id, created_at) VALUES (?, ?, ?) \
         ON CONFLICT(user_id, match_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(match_id)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn unsubscribe(pool: &SqlitePool, user_id: i64, match_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM match_notifications WHERE user_id = ? AND match_id = ?")
        .bind(user_id)
        .bind(match_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<i64>, AppError> {
    let ids = sqlx::query_scalar::<_, i64>(
        "SELECT match_id FROM match_notifications WHERE user_id = ? ORDER BY match_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

pub async fn update_settings(
    pool: &SqlitePool,
    user_id: i64,
    email_enabled: bool,
    telegram_enabled: bool,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET notify_email = ?, notify_telegram = ? WHERE id = ?")
        .bind(email_enabled)
        .bind(telegram_enabled)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn process(state: &AppState, changes: Vec<Change>) {
    // if there is no change, we don't do anything here
    for change in changes {
        // we only care about scheduled matches
        let Change::Scheduled {
            id,
            team,
            opponent,
            kickoff,
        } = change;
        // process the match
        process_one(state, id, team.to_string(), opponent.to_string(), kickoff).await;
    }
}

async fn process_one(
    state: &AppState,
    match_id: u32,
    team: String,
    opponent: String,
    kickoff: String,
) {
    // get the subscribers for this match id
    let subscribers: Vec<(i64, String, Option<String>, bool, bool)> = match sqlx::query_as(
        "SELECT mn.user_id, u.email, u.telegram_id, u.notify_email, u.notify_telegram \
         FROM match_notifications mn JOIN users u ON u.id = mn.user_id WHERE mn.match_id = ?",
    )
    .bind(i64::from(match_id))
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("failed to load subscribers for match {match_id}: {e}");
            return;
        }
    };

    // if there is no subscribers, we don't do anything here
    if subscribers.is_empty() {
        return;
    }

    // send the alert to all the subscribers
    let alert = MatchAlert {
        team,
        opponent,
        kickoff,
    };
    // for each subscriber, notify through the enabled channels
    for (user_id, email, telegram_id, notify_email, notify_telegram) in &subscribers {
        // If noifify email is enabled, send the email
        if *notify_email {
            if let Err(e) = state.mailer.send_match_alert(email, &alert).await {
                tracing::warn!("failed to send match alert email for user {user_id}: {e}");
            }
        }
        // If notify telegram is enabled, send the telegram message
        if *notify_telegram {
            // Check that the user has a telegram id
            if let Some(chat_id) = telegram_id {
                let text = format!(
                    "Match scheduled: {} vs {} on {}.",
                    alert.team, alert.opponent, alert.kickoff
                );
                if let Err(e) = state.telegram.send_message(chat_id, &text).await {
                    tracing::warn!(
                        "failed to send match alert on telegram for user {user_id}: {e}"
                    );
                }
            }
        }
    }
    // clear the notifications for this match in the database
    if let Err(e) = sqlx::query("DELETE FROM match_notifications WHERE match_id = ?")
        .bind(i64::from(match_id))
        .execute(&state.pool)
        .await
    {
        tracing::warn!("failed to clear notifications for match {match_id}: {e}");
    }
}
