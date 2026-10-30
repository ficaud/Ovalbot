//! Shared foundations: data models persisted in SQLite and their API views.
//!
//! The `User` row is the single source of truth; [`UserView`] is the safe
//! projection served over HTTP (never leaks the password hash or the raw
//! Telegram id).

use serde::Serialize;
use sqlx::FromRow;

/// Privilege level of an account, stored in the `users.role` column (`admin`
/// unlocks the admin-only routes; `user` is the default at registration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    /// The SQLite representation of the role, as written by the migrations
    /// (`'admin'` / `'user'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }
}

/// Whether an account can currently authenticate and act on the site
/// (`users.status`). `Waiting` is the waitlist state until an `active` user
/// slot frees up or an admin activates the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Status {
    Active,
    Waiting,
}

impl Status {
    /// The SQLite representation of the status, as written by the migrations
    /// (`'active'` / `'waiting'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Waiting => "waiting",
        }
    }
}

/// A `users` row: the persisted account, including the secret and notification
/// state (`FromRow` binds it to `SELECT * FROM users`-shaped queries).
#[derive(Debug, Clone, FromRow)]
pub struct User {
    /// Primary key of the `users` table.
    pub id: i64,
    /// Lower-cased registration email, unique across accounts.
    pub email: String,
    /// Argon2id hash (`password_hash` column) — never sent over the wire.
    pub password_hash: String,
    /// Active vs waitlist account.
    pub status: Status,
    /// Privilege level (`admin` unlocks admin routes).
    pub role: Role,
    /// Registration timestamp (RFC 3339, UTC).
    pub created_at: String,
    /// Manual or automatic activation timestamp; `None` while waiting.
    pub activated_at: Option<String>,
    /// Anti-duplicate marker: set when the welcome email left for this user.
    pub welcome_email_sent_at: Option<String>,
    /// Raw Telegram chat id when the account is linked, `None` otherwise —
    /// exposed to the API only as a boolean (see [`UserView::telegram_linked`]).
    pub telegram_id: Option<String>,
    /// Per-channel preference: receive match alerts by email.
    pub notify_email: bool,
    /// Per-channel preference: receive match alerts by Telegram.
    pub notify_telegram: bool,
    /// The billetterie account email the user saved for the (Phase 4B)
    /// automatic add-to-cart; `None` until set. Readable back by its owner
    /// (see [`UserView::shop_email`]), the password never is.
    pub shop_email: Option<String>,
    /// The AES-GCM encrypted billetterie password (`shop_password_cipher`
    /// column, hex `nonce || cipher+tag`) — decrypted only inside the checkout
    /// flow, never serialized anywhere.
    pub shop_password_cipher: Option<String>,
}

/// A `sessions` row: a login cookie's server-side counterpart.
#[derive(Debug, Clone, FromRow)]
pub struct Session {
    /// Random opaque token sent as a cookie value (primary key).
    pub id: String,
    /// Account the session belongs to (`ON DELETE CASCADE` on the user).
    pub user_id: i64,
    /// Login timestamp (RFC 3339, UTC).
    pub created_at: String,
    /// Hard expiry used for cleanup; sessions past it are rejected on use.
    pub expires_at: String,
}

impl Session {
    /// Whether the session token can no longer be used.
    pub fn is_expired(&self) -> bool {
        crate::common::time::is_past(&self.expires_at)
    }
}

/// The account as serialized to the API: the [`User`] row stripped of its
/// password hash, with the Telegram link reduced to a boolean so the raw chat
/// id never leaves the server.
#[derive(Debug, Clone, Serialize)]
pub struct UserView {
    pub id: i64,
    pub email: String,
    pub status: Status,
    pub role: Role,
    /// Registration timestamp (RFC 3339, UTC).
    pub created_at: String,
    /// Manual or automatic activation timestamp; `None` while waiting.
    pub activated_at: Option<String>,
    /// `true` when a Telegram chat is linked (raw id lives only in the DB).
    pub telegram_linked: bool,
    /// Mirror of the per-channel preference rows.
    pub notify_email: bool,
    pub notify_telegram: bool,
    /// The saved billetterie account email, when the user configured the
    /// shop-credentials vault (`None` = not configured; the password never
    /// leaves the server in any form).
    pub shop_email: Option<String>,
}

impl From<User> for UserView {
    /// Strips the password hash; `telegram_linked` is derived, not copied.
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
            shop_email: u.shop_email,
        }
    }
}
