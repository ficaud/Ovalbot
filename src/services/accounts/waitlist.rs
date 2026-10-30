//! Waitlist admission logic: how many accounts may be `active` and who is
//! promoted next when a slot frees up.
//!
//! Registration uses [`status_for`] to either admit or queue the account;
//! `promote_next_waiting` (called by admin activation/deletion flows) fills
//! the first stale slot.

use sqlx::SqlitePool;

use crate::common::error::AppError;
use crate::common::models::{Status, User};

/// Maximum number of simultaneously `active` accounts (the handful of beta
/// seats the site can serve).
pub const ACTIVE_LIMIT: i64 = 10;

/// Maps the current active count to the status of a brand-new registration:
/// `Active` while under [`ACTIVE_LIMIT`], `Waiting` from there on.
///
/// # Arguments
/// * `active_count` — number of users with `status = 'active'`.
///
/// # Returns
/// The status to give the new account.
pub fn status_for(active_count: i64) -> Status {
    if active_count < ACTIVE_LIMIT {
        Status::Active
    } else {
        Status::Waiting
    }
}

/// Promotes the longest-waiting account from the waitlist: the first
/// `status = 'waiting'` row in creation order is switched to `active` with
/// `activated_at = now`.
///
/// # Returns
/// The promoted user (already updated in DB), or `None` when no one is
/// waiting — the caller then sends no welcome email.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn promote_next_waiting(pool: &SqlitePool) -> Result<Option<User>, AppError> {
    let now = crate::common::time::now();
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at, telegram_id, notify_email, notify_telegram, shop_email, shop_password_cipher \
         FROM users WHERE status = 'waiting' ORDER BY created_at ASC, id ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    let Some(user) = user else {
        return Ok(None);
    };

    sqlx::query("UPDATE users SET status = 'active', activated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(user.id)
        .execute(pool)
        .await?;

    Ok(Some(user))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `status_for` returns `Active` for any active count below the limit.
    #[test]
    fn active_below_limit() {
        for count in 0..ACTIVE_LIMIT {
            assert_eq!(status_for(count), Status::Active);
        }
    }

    /// `status_for` returns `Waiting` once the active count reaches the limit.
    #[test]
    fn waiting_at_and_above_limit() {
        assert_eq!(status_for(ACTIVE_LIMIT), Status::Waiting);
        assert_eq!(status_for(ACTIVE_LIMIT + 1), Status::Waiting);
    }
}
