use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Status {
    Active,
    Waiting,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Waiting => "waiting",
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub status: Status,
    pub role: Role,
    pub created_at: String,
    pub activated_at: Option<String>,
    pub welcome_email_sent_at: Option<String>,
    pub telegram_id: Option<String>,
    pub notify_email: bool,
    pub notify_telegram: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct Session {
    pub id: String,
    pub user_id: i64,
    pub created_at: String,
    pub expires_at: String,
}

impl Session {
    pub fn is_expired(&self) -> bool {
        crate::common::time::is_past(&self.expires_at)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UserView {
    pub id: i64,
    pub email: String,
    pub status: Status,
    pub role: Role,
    pub created_at: String,
    pub activated_at: Option<String>,
    pub telegram_linked: bool,
    pub notify_email: bool,
    pub notify_telegram: bool,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            email: u.email,
            status: u.status,
            role: u.role,
            created_at: u.created_at,
            activated_at: u.activated_at,
            telegram_linked: u.telegram_id.is_some(),
            notify_email: u.notify_email,
            notify_telegram: u.notify_telegram,
        }
    }
}
