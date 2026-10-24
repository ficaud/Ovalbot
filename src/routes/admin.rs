use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::common::error::AppError;
use crate::common::models::{Role, User, UserView};
use crate::services::accounts::auth::AdminUser;
use crate::services::accounts::waitlist::promote_next_waiting;
use crate::services::channels::mailer::send_welcome_if_unsent;
use crate::services::state::AppState;

/// `GET /api/admin/users` — every account as [`UserView`] (admin only),
/// ordered by id.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Vec<UserView>>, AppError> {
    let users = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at, telegram_id, notify_email, notify_telegram FROM users ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users.into_iter().map(UserView::from).collect()))
}

/// `POST /api/admin/users/{id}/activate` — force-activates a waitlist account
/// and sends its welcome email if none went out yet.
///
/// # Errors
/// `AppError::NotFound` when the id is unknown, `AppError::Sqlx` on DB failure.
pub async fn activate_user(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let now = crate::common::time::now();
    let result = sqlx::query("UPDATE users SET status = 'active', activated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    send_welcome_if_unsent(state.mailer.as_ref(), &state.pool, id, &email).await;
    Ok(StatusCode::OK)
}

/// `DELETE /api/admin/users/{id}` — deletes a non-admin account (cascade on
/// sessions/tickets via FK), its sessions, then promotes the next waiting
/// account into the freed slot (with its welcome email).
///
/// # Errors
/// `AppError::NotFound` (unknown id), `AppError::Forbidden` (an admin cannot
/// delete another admin), `AppError::Sqlx` on DB failure.
pub async fn delete_user(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at, telegram_id, notify_email, notify_telegram FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if user.role == Role::Admin {
        return Err(AppError::Forbidden);
    }

    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;

    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;

    let promoted = promote_next_waiting(&state.pool).await?;
    if let Some(user) = promoted {
        send_welcome_if_unsent(state.mailer.as_ref(), &state.pool, user.id, &user.email).await;
    }
    Ok(StatusCode::OK)
}
