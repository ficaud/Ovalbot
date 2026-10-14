use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

pub const LINK_TTL_MINUTES: i64 = 15;

// ============================== TELEGRAM CLIENT ==============================

#[derive(Debug, thiserror::Error)]
pub enum TelegramError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("telegram api error: {0}")]
    Api(String),
}

#[async_trait]
pub trait Telegram: Send + Sync {
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), TelegramError>;
}

#[derive(Clone, Debug, Default)]
pub struct TelegramConfig {
    pub bot_username: Option<String>,
    pub webhook_secret: Option<String>,
}

/// Telegram client implementation of the Telegram trait,
/// containing a reqwest client and a bot token.
#[derive(Clone)]
pub struct TelegramClient {
    client: reqwest::Client,
    token: String,
}

#[derive(Serialize)]
struct OutgoingMessage<'a> {
    chat_id: &'a str,
    text: &'a str,
}

impl TelegramClient {
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.token)
    }
}

/// Telegram client implementation of the Telegram trait.
#[async_trait]
impl Telegram for TelegramClient {
    /// Sends a message to the specified chat.
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), TelegramError> {
        // Create a new outgoing message struct with the chat ID and text.
        let response = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&OutgoingMessage { chat_id, text })
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(TelegramError::Api(response.status().to_string()));
        }
        Ok(())
    }
}

// ============================== TELEGRAM NOOP =================================

/// A no-op implementation of the Telegram trait.
pub struct NoopTelegram;

#[async_trait]
impl Telegram for NoopTelegram {
    /// No operation implementation of the send message method that only returns Ok(()).
    async fn send_message(&self, _chat_id: &str, _text: &str) -> Result<(), TelegramError> {
        Ok(())
    }
}

/// Returns true if the telegram integration is enabled by checking the environment variables.
fn enabled() -> bool {
    std::env::var("TELEGRAM_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// Returns a new instance of the Telegram trait based on the environment variables.
pub fn from_env() -> Arc<dyn Telegram> {
    // If the telegram integration is disabled, return a no-op implementation.
    if !enabled() {
        return Arc::new(NoopTelegram);
    }
    // Create a new TelegramClient instance with the bot token.
    let token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    Arc::new(TelegramClient::new(token))
}

/// Returns a TelegramConfig struct based on the environment variables.
pub fn config_from_env() -> TelegramConfig {
    // If the telegram integration is disabled, return a default config.
    if !enabled() {
        return TelegramConfig::default();
    }
    // Create a new TelegramConfig struct with the bot username and webhook secret.
    TelegramConfig {
        bot_username: std::env::var("TELEGRAM_BOT_USERNAME").ok(),
        webhook_secret: std::env::var("TELEGRAM_WEBHOOK_SECRET").ok(),
    }
}

/// Registers the webhook with the Telegram API.
pub async fn register_webhook() {
    // If the telegram integration is disabled, return.
    if !enabled() {
        return;
    }
    // Get the bot token and webhook URL from the environment variables.
    let token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let url = match std::env::var("TELEGRAM_WEBHOOK_URL") {
        Ok(url) => url,
        Err(_) => {
            tracing::warn!("TELEGRAM_WEBHOOK_URL not set, skipping webhook registration");
            return;
        }
    };
    // Get the webhook secret from the environment variables.
    let secret = std::env::var("TELEGRAM_WEBHOOK_SECRET").unwrap_or_default();
    let client = TelegramClient::new(token);

    // Create the setWebhook request parameters.
    let mut params = vec![("url", url)];
    if !secret.is_empty() {
        params.push(("secret_token", secret));
    }

    // Send the setWebhook request to the Telegram API.
    // from doc : https://core.telegram.org/bots/api#making-requests
    match client
        .client
        .post(client.api_url("setWebhook"))
        .form(&params)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            tracing::info!("telegram webhook registered");
        }
        Ok(response) => tracing::warn!("telegram setWebhook failed: {}", response.status()),
        Err(e) => tracing::warn!("telegram setWebhook error: {e}"),
    }
}

// ============================== TELEGRAM UPDATES =============================

#[derive(Debug, Deserialize)]
pub struct Update {
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub chat: Chat,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
}

impl Update {
    /// Returns the `(chat_id, token)` pair when the update is a `/start <token>` command.
    pub fn start_token(&self) -> Option<(String, String)> {
        let message = self.message.as_ref()?;
        let text = message.text.as_deref()?;
        let mut parts = text.split_whitespace();
        let command = parts.next()?;
        if command != "/start" && !command.starts_with("/start@") {
            return None;
        }
        let token = parts.next()?.to_string();
        Some((message.chat.id.to_string(), token))
    }
}

/// Constant-time comparison of the webhook secret token.
pub fn secret_matches(expected: &str, provided: &str) -> bool {
    if expected.is_empty() || expected.len() != provided.len() {
        return false;
    }
    expected
        .bytes()
        .zip(provided.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Creates a new link token for the given user.
pub async fn create_link_token(pool: &SqlitePool, user_id: i64) -> Result<String, AppError> {
    let token = crate::auth::generate_session_token();
    let now = crate::time::now();
    let expires_at = crate::time::in_minutes(LINK_TTL_MINUTES);

    sqlx::query("DELETE FROM telegram_link_tokens WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO telegram_link_tokens (token, user_id, created_at, expires_at) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&token)
    .bind(user_id)
    .bind(&now)
    .bind(&expires_at)
    .execute(pool)
    .await?;

    Ok(token)
}

/// Consumes a link token and binds the chat to the user. Returns `false` when the
/// token is unknown, expired, or already bound to another chat.
pub async fn consume_link_token(
    pool: &SqlitePool,
    token: &str,
    chat_id: &str,
) -> Result<bool, AppError> {
    let row = sqlx::query_as::<_, (i64, String)>(
        "SELECT user_id, expires_at FROM telegram_link_tokens WHERE token = ?",
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;

    let Some((user_id, expires_at)) = row else {
        return Ok(false);
    };

    // Unlink the chat from the user.
    sqlx::query("DELETE FROM telegram_link_tokens WHERE token = ?")
        .bind(token)
        .execute(pool)
        .await?;

    // Check if the token is expired.
    if crate::common::time::is_past(&expires_at) {
        return Ok(false);
    }

    // Update the user's telegram id.
    match sqlx::query("UPDATE users SET telegram_id = ? WHERE id = ?")
        .bind(chat_id)
        .bind(user_id)
        .execute(pool)
        .await
    {
        Ok(_) => Ok(true),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Unlinks the chat from the user.
pub async fn unlink(pool: &SqlitePool, user_id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET telegram_id = NULL WHERE id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Returns a deep link to the bot with the given username and token.
pub fn deep_link(bot_username: &str, token: &str) -> String {
    format!("https://t.me/{bot_username}?start={token}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(text: &str, chat_id: i64) -> Update {
        Update {
            message: Some(Message {
                chat: Chat { id: chat_id },
                text: Some(text.to_string()),
            }),
        }
    }

    /// `/start <token>` yields the chat id and the token.
    #[test]
    fn start_command_extracts_token() {
        let (chat_id, token) = update("/start abc123", 42).start_token().unwrap();
        assert_eq!(chat_id, "42");
        assert_eq!(token, "abc123");
    }

    /// A bare `/start` without a token yields nothing.
    #[test]
    fn bare_start_has_no_token() {
        assert!(update("/start", 42).start_token().is_none());
    }

    /// The `@BotName` suffix on the command is tolerated.
    #[test]
    fn start_command_with_bot_suffix() {
        let (_, token) = update("/start@OvalBot abc123", 42).start_token().unwrap();
        assert_eq!(token, "abc123");
    }

    /// Non-command messages are ignored.
    #[test]
    fn non_start_message_is_ignored() {
        assert!(update("/help", 42).start_token().is_none());
        assert!(Update { message: None }.start_token().is_none());
    }

    /// `secret_matches` accepts the exact secret and rejects everything else.
    #[test]
    fn secret_comparison() {
        assert!(secret_matches("s3cr3t", "s3cr3t"));
        assert!(!secret_matches("s3cr3t", "s3cr3T"));
        assert!(!secret_matches("s3cr3t", "short"));
        assert!(!secret_matches("", ""));
        assert!(!secret_matches("s3cr3t", ""));
    }

    /// The deep link embeds the bot username and the token.
    #[test]
    fn deep_link_contains_username_and_token() {
        assert_eq!(
            deep_link("OvalBot", "tok"),
            "https://t.me/OvalBot?start=tok"
        );
    }
}
