use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{Status, User};

pub const ACTIVE_LIMIT: i64 = 10;

pub fn status_for(active_count: i64) -> Status {
    if active_count < ACTIVE_LIMIT {
        Status::Active
    } else {
        Status::Waiting
    }
}

pub async fn promote_next_waiting(pool: &SqlitePool) -> Result<Option<User>, AppError> {
    let now = crate::time::now();
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at \
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
