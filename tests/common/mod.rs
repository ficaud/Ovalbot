#![allow(dead_code)]

use std::sync::Mutex;

use async_trait::async_trait;
use ovalbot::services::mailer::{MailError, Mailer, MatchAlert};
use ovalbot::services::telegram::{Telegram, TelegramError};
use sqlx::SqlitePool;

pub struct RecordingMailer {
    pub welcome: Mutex<Vec<String>>,
    pub alerts: Mutex<Vec<(String, MatchAlert)>>,
}

impl RecordingMailer {
    pub fn new() -> Self {
        Self {
            welcome: Mutex::new(Vec::new()),
            alerts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Mailer for RecordingMailer {
    async fn send_welcome(&self, email: &str) -> Result<(), MailError> {
        self.welcome.lock().unwrap().push(email.to_string());
        Ok(())
    }

    async fn send_match_alert(&self, email: &str, alert: &MatchAlert) -> Result<(), MailError> {
        self.alerts
            .lock()
            .unwrap()
            .push((email.to_string(), alert.clone()));
        Ok(())
    }
}

pub struct RecordingTelegram {
    pub messages: Mutex<Vec<(String, String)>>,
}

impl RecordingTelegram {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Telegram for RecordingTelegram {
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), TelegramError> {
        self.messages
            .lock()
            .unwrap()
            .push((chat_id.to_string(), text.to_string()));
        Ok(())
    }
}

pub async fn insert_active_user(pool: &SqlitePool, email: &str) -> i64 {
    let now = ovalbot::common::time::now();
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
