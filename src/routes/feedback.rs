//! Feedback routes: the `/feedback` page's submission endpoint plus the admin
//! read side. Reports are stored first (the source of truth), then emailed
//! best-effort to every admin — a mail failure never loses a report.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::common::error::AppError;
use crate::services::accounts::auth::{ActiveUser, AdminUser};
use crate::services::channels::alerts::ReportAlert;
use crate::services::feedback::{self, FeedbackInput, FeedbackReportView};
use crate::services::state::AppState;

/// `POST /api/feedback` — stores the current active user's bug report or
/// suggestion, then notifies every admin by email (best-effort: a send
/// failure is logged, the report stays stored).
///
/// # Returns
/// `201` with the created [`FeedbackReportView`].
///
/// # Errors
/// `AppError::InvalidFeedback` (400) on an invalid payload, `AppError::Sqlx`
/// on DB failure.
pub async fn submit_report(
    State(state): State<AppState>,
    auth: ActiveUser,
    Json(input): Json<FeedbackInput>,
) -> Result<Response, AppError> {
    let view = feedback::submit(&state.pool, auth.0.user.id, &input).await?;
    notify_admins(&state, &view).await;
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

/// `GET /api/admin/feedback` — every stored report, newest first, with the
/// author's email (admin only).
///
/// # Errors
/// `AppError::Sqlx` on DB failure.
pub async fn list_reports(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Vec<FeedbackReportView>>, AppError> {
    Ok(Json(feedback::list_all(&state.pool).await?))
}

/// Emails every admin about one new report. Best-effort by design: each send
/// failure is logged and skipped, the stored report is never rolled back.
async fn notify_admins(state: &AppState, view: &FeedbackReportView) {
    let alert = ReportAlert {
        reporter_email: view.reporter_email.clone(),
        kind: view.kind.as_str().to_string(),
        message: view.message.clone(),
    };
    match feedback::admin_emails(&state.pool).await {
        Ok(emails) => {
            for email in emails {
                if let Err(err) = state.mailer.send_report_alert(&email, &alert).await {
                    tracing::warn!("failed to email the feedback report to {email}: {err}");
                }
            }
        }
        Err(err) => tracing::warn!("failed to load the admin emails for a feedback report: {err}"),
    }
}
