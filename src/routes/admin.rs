use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::{Role, User, UserView};
use crate::state::AppState;
use crate::waitlist::promote_next_waiting;

pub async fn list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Vec<UserView>>, AppError> {
    let users = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at FROM users ORDER BY id",
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
    let now = crate::time::now();
    let result = sqlx::query("UPDATE users SET status = 'active', activated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::OK)
}

pub async fn delete_user(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at FROM users WHERE id = ?",
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

    promote_next_waiting(&state.pool).await?;
    Ok(StatusCode::OK)
}
