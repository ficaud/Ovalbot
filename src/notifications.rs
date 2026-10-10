use calendar::Change;
use sqlx::SqlitePool;

use crate::error::AppError;
use crate::mailer::MatchAlert;
use crate::state::AppState;

pub async fn subscribe(pool: &SqlitePool, user_id: i64, match_id: i64) -> Result<(), AppError> {
    let now = crate::time::now();
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

pub async fn process(state: &AppState, changes: Vec<Change>) {
    for change in changes {
        let Change::Scheduled {
            id,
            team,
            opponent,
            kickoff,
        } = change;
        process_one(state, id, team.to_string(), opponent.to_string(), kickoff).await;
    }
}

async fn process_one(
    state: &AppState,
    match_id: u16,
    team: String,
    opponent: String,
    kickoff: String,
) {
    let subscribers: Vec<(i64, String)> = match sqlx::query_as(
        "SELECT mn.user_id, u.email FROM match_notifications mn \
         JOIN users u ON u.id = mn.user_id WHERE mn.match_id = ?",
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

    if subscribers.is_empty() {
        return;
    }

    let alert = MatchAlert {
        team,
        opponent,
        kickoff,
    };
    for (user_id, email) in &subscribers {
        if let Err(e) = state.mailer.send_match_alert(email, &alert).await {
            tracing::warn!("failed to send match alert for user {user_id}: {e}");
        }
    }

    if let Err(e) = sqlx::query("DELETE FROM match_notifications WHERE match_id = ?")
        .bind(i64::from(match_id))
        .execute(&state.pool)
        .await
    {
        tracing::warn!("failed to clear notifications for match {match_id}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use calendar::TeamType;

    use super::*;
    use crate::calendar::CalendarService;
    use crate::mailer::Mailer;

    struct RecordingMailer(std::sync::Mutex<Vec<(String, MatchAlert)>>);

    #[async_trait::async_trait]
    impl Mailer for RecordingMailer {
        async fn send_welcome(&self, _email: &str) -> Result<(), crate::mailer::MailError> {
            Ok(())
        }

        async fn send_match_alert(
            &self,
            email: &str,
            alert: &MatchAlert,
        ) -> Result<(), crate::mailer::MailError> {
            self.0
                .lock()
                .unwrap()
                .push((email.to_string(), alert.clone()));
            Ok(())
        }
    }

    fn state(pool: SqlitePool, mailer: Arc<dyn Mailer>) -> AppState {
        AppState {
            pool,
            mailer,
            calendar: Arc::new(CalendarService::new()),
        }
    }

    async fn insert_active_user(pool: &SqlitePool, email: &str) -> i64 {
        let now = crate::time::now();
        sqlx::query(
            "INSERT INTO users (email, password_hash, status, role, created_at, activated_at) \
             VALUES (?, 'x', 'active', 'user', ?, ?)",
        )
        .bind(email)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
    }

    #[sqlx::test]
    async fn subscribe_is_idempotent(pool: SqlitePool) {
        let user = insert_active_user(&pool, "a@example.com").await;

        subscribe(&pool, user, 5).await.unwrap();
        subscribe(&pool, user, 5).await.unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM match_notifications WHERE user_id = ? AND match_id = ?",
        )
        .bind(user)
        .bind(5)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[sqlx::test]
    async fn unsubscribe_is_a_noop_for_unknown_match(pool: SqlitePool) {
        let user = insert_active_user(&pool, "a@example.com").await;

        unsubscribe(&pool, user, 42).await.unwrap();

        let ids = list_for_user(&pool, user).await.unwrap();
        assert!(ids.is_empty());
    }

    #[sqlx::test]
    async fn list_returns_subscribed_match_ids(pool: SqlitePool) {
        let user = insert_active_user(&pool, "a@example.com").await;
        subscribe(&pool, user, 3).await.unwrap();
        subscribe(&pool, user, 1).await.unwrap();

        let ids = list_for_user(&pool, user).await.unwrap();
        assert_eq!(ids, vec![1, 3]);
    }

    #[sqlx::test]
    async fn process_emails_subscribers_once_and_clears_rows(pool: SqlitePool) {
        let user_a = insert_active_user(&pool, "a@example.com").await;
        let user_b = insert_active_user(&pool, "b@example.com").await;
        subscribe(&pool, user_a, 7).await.unwrap();
        subscribe(&pool, user_b, 7).await.unwrap();
        subscribe(&pool, user_b, 8).await.unwrap();

        let mailer = Arc::new(RecordingMailer(std::sync::Mutex::new(Vec::new())));
        let state = state(pool.clone(), mailer.clone());

        process(
            &state,
            vec![Change::Scheduled {
                id: 7,
                team: TeamType::StadeRochelais,
                opponent: TeamType::RcToulon,
                kickoff: "samedi 10 octobre 14h30".to_string(),
            }],
        )
        .await;

        let sent = mailer.0.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
        let emails: Vec<&str> = sent.iter().map(|(email, _)| email.as_str()).collect();
        assert!(emails.contains(&"a@example.com"));
        assert!(emails.contains(&"b@example.com"));
        assert_eq!(sent[0].1.team, "Stade Rochelais");
        assert_eq!(sent[0].1.opponent, "RC Toulon");

        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM match_notifications WHERE match_id = 7")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);

        let unrelated: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM match_notifications WHERE match_id = 8")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unrelated, 1);
    }

    #[sqlx::test]
    async fn process_without_subscribers_sends_nothing(pool: SqlitePool) {
        let mailer = Arc::new(RecordingMailer(std::sync::Mutex::new(Vec::new())));
        let state = state(pool.clone(), mailer.clone());

        process(
            &state,
            vec![Change::Scheduled {
                id: 9,
                team: TeamType::StadeRochelais,
                opponent: TeamType::RcToulon,
                kickoff: "samedi 10 octobre 14h30".to_string(),
            }],
        )
        .await;

        assert!(mailer.0.lock().unwrap().is_empty());
    }
}
