use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::common::error::AppError;
use crate::common::models::{Role, User, UserView};
use crate::services::auth::AdminUser;
use crate::services::mailer::send_welcome_if_unsent;
use crate::services::state::AppState;
use crate::services::waitlist::promote_next_waiting;

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
