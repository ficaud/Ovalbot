use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("resend api error: {0}")]
    Api(String),
}

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send_welcome(&self, email: &str) -> Result<(), MailError>;
}

const WELCOME_SUBJECT: &str = "Welcome to OvalBot";

const WELCOME_TEXT: &str = "Your OvalBot account is now active. \
You can now log in and view the Stade Rochelais home matches.";

const WELCOME_HTML: &str = "<p>Your OvalBot account is now active.</p>\
<p>You can now log in and view the Stade Rochelais home matches.</p>";

#[derive(Serialize)]
struct WelcomeEmail<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    html: &'a str,
    text: &'a str,
}

#[derive(Clone)]
pub struct ResendMailer {
    client: reqwest::Client,
    api_key: String,
    from: String,
}

impl ResendMailer {
    pub fn new(api_key: String, from: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            from,
        }
    }
}

#[async_trait]
impl Mailer for ResendMailer {
    async fn send_welcome(&self, email: &str) -> Result<(), MailError> {
        let payload = WelcomeEmail {
            from: &self.from,
            to: email,
            subject: WELCOME_SUBJECT,
            html: WELCOME_HTML,
            text: WELCOME_TEXT,
        };
        let response = self
            .client
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(MailError::Api(response.status().to_string()));
        }
        Ok(())
    }
}

pub struct NoopMailer;

#[async_trait]
impl Mailer for NoopMailer {
    async fn send_welcome(&self, _email: &str) -> Result<(), MailError> {
        Ok(())
    }
}

pub fn from_env() -> Arc<dyn Mailer> {
    let enabled = std::env::var("RESEND_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !enabled {
        return Arc::new(NoopMailer);
    }
    let api_key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    let from = std::env::var("RESEND_FROM")
        .unwrap_or_else(|_| "OvalBot <onboarding@resend.dev>".to_string());
    Arc::new(ResendMailer::new(api_key, from))
}

pub async fn send_welcome_if_unsent(
    mailer: &dyn Mailer,
    pool: &SqlitePool,
    user_id: i64,
    email: &str,
) {
    let already_sent = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE id = ? AND welcome_email_sent_at IS NOT NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    {
        Ok(count) => count > 0,
        Err(e) => {
            tracing::warn!("failed to check welcome email status for user {user_id}: {e}");
            return;
        }
    };

    if already_sent {
        return;
    }

    match mailer.send_welcome(email).await {
        Ok(()) => {
            let now = crate::time::now();
            if let Err(e) = sqlx::query("UPDATE users SET welcome_email_sent_at = ? WHERE id = ?")
                .bind(&now)
                .bind(user_id)
                .execute(pool)
                .await
            {
                tracing::warn!("failed to record welcome email for user {user_id}: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("failed to send welcome email to {email}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct RecordingMailer(Mutex<Vec<String>>);

    #[async_trait]
    impl Mailer for RecordingMailer {
        async fn send_welcome(&self, email: &str) -> Result<(), MailError> {
            self.0.lock().unwrap().push(email.to_string());
            Ok(())
        }
    }

    struct FailingMailer;

    #[async_trait]
    impl Mailer for FailingMailer {
        async fn send_welcome(&self, _email: &str) -> Result<(), MailError> {
            Err(MailError::Api("boom".to_string()))
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
    async fn sends_welcome_once_then_skips(pool: SqlitePool) {
        let id = insert_active_user(&pool, "a@example.com").await;
        let mailer = RecordingMailer(Mutex::new(Vec::new()));

        send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;
        send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;

        let sent = mailer.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0], "a@example.com");
    }

    #[sqlx::test]
    async fn failed_send_leaves_marker_unset(pool: SqlitePool) {
        let id = insert_active_user(&pool, "a@example.com").await;
        let mailer = FailingMailer;

        send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE id = ? AND welcome_email_sent_at IS NOT NULL",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }
}
