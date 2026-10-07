use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::Status;

pub const ACTIVE_LIMIT: i64 = 10;

pub fn status_for(active_count: i64) -> Status {
    if active_count < ACTIVE_LIMIT {
        Status::Active
    } else {
        Status::Waiting
    }
}

pub async fn promote_next_waiting(pool: &SqlitePool) -> Result<(), AppError> {
    let now = crate::time::now();
    sqlx::query(
        "UPDATE users SET status = 'active', activated_at = ? \
         WHERE id = (SELECT id FROM users WHERE status = 'waiting' \
         ORDER BY created_at ASC, id ASC LIMIT 1)",
    )
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_below_limit() {
        for count in 0..ACTIVE_LIMIT {
            assert_eq!(status_for(count), Status::Active);
        }
    }

    #[test]
    fn waiting_at_and_above_limit() {
        assert_eq!(status_for(ACTIVE_LIMIT), Status::Waiting);
        assert_eq!(status_for(ACTIVE_LIMIT + 1), Status::Waiting);
    }
}
