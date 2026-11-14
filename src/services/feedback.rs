//! Feedback: the bug reports and improvement suggestions active users submit
//! from the `/feedback` page, persisted in SQLite (`feedback_reports`).
//!
//! Deliberately minimal by design (v1): no status tracking, no attachments, no
//! admin answer — a report is stored once, emailed best-effort to the admins
//! (see [`crate::routes::feedback`]) and read back by the admin page via
//! [`list_all`]. Deleting a user cascades to their reports.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::common::error::AppError;

// ============================== FEEDBACK INPUT ==============================

/// Defensive upper bound for a report message (chars, not bytes — the bound
/// the frontend textarea enforces too).
const MAX_MESSAGE_CHARS: usize = 4_000;

/// One submitted report, tagged on the feedback kind (`{"kind": ...}`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FeedbackInput {
    /// Something is broken or behaves unexpectedly.
    Bug { message: String },
    /// An idea to improve OvalBot.
    Suggestion { message: String },
}

impl FeedbackInput {
    /// Validates the payload against the bounds the frontend enforces
    /// (message trimmed-non-empty and at most [`MAX_MESSAGE_CHARS`] chars).
    ///
    /// # Returns
    /// `true` when the report may be stored; the route answers 400 otherwise.
    fn is_valid(&self) -> bool {
        let (Self::Bug { message } | Self::Suggestion { message }) = self;
        !message.trim().is_empty() && message.chars().count() <= MAX_MESSAGE_CHARS
    }

    /// Flattens the variant into the `feedback_reports` columns
    /// `(kind, message)`.
    ///
    /// # Returns
    /// The column pair to bind in the insert statement.
    fn columns(&self) -> (FeedbackKind, &str) {
        match self {
            Self::Bug { message } => (FeedbackKind::Bug, message),
            Self::Suggestion { message } => (FeedbackKind::Suggestion, message),
        }
    }
}

// ============================== FEEDBACK TYPES ==============================

/// Feedback kind of a stored report — serialized as `"bug"`/`"suggestion"` and
/// stored with the same spelling (sqlx `rename_all`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum FeedbackKind {
    /// Something is broken or behaves unexpectedly.
    Bug,
    /// An idea to improve OvalBot.
    Suggestion,
}

impl FeedbackKind {
    /// The lowercase SQLite representation of the kind, as written by the
    /// migration and reused in log fields.
    pub fn as_str(self) -> &'static str {
        match self {
            FeedbackKind::Bug => "bug",
            FeedbackKind::Suggestion => "suggestion",
        }
    }
}

/// One stored report as serialized by `POST /api/feedback` (creation receipt)
/// and `GET /api/admin/feedback` (the admin reads the author's email to be
/// able to follow up).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct FeedbackReportView {
    /// Stored row id (the only tracking handle; reports are immutable).
    pub id: i64,
    /// Bug report or improvement suggestion.
    pub kind: FeedbackKind,
    /// The user's free-form message, as submitted (trimmed only for
    /// emptiness, stored verbatim).
    pub message: String,
    /// Author account email (`JOIN users`), readable back by admins only.
    pub reporter_email: String,
    /// Submission timestamp (RFC 3339).
    pub created_at: String,
}

// ============================== SERVICE ==============================

/// Stores one report and returns the created view (with the author's email).
///
/// # Errors
/// `AppError::InvalidFeedback` on an invalid payload, `AppError::Sqlx` on DB
/// failure.
pub async fn submit(
    pool: &SqlitePool,
    user_id: i64,
    input: &FeedbackInput,
) -> Result<FeedbackReportView, AppError> {
    if !input.is_valid() {
        return Err(AppError::InvalidFeedback);
    }

    let now = crate::common::time::now();
    let (kind, message) = input.columns();

    let view = sqlx::query_as::<_, FeedbackReportView>(
        "INSERT INTO feedback_reports (user_id, kind, message, created_at) \
         VALUES (?, ?, ?, ?) \
         RETURNING id, kind, message, \
                   (SELECT email FROM users WHERE id = feedback_reports.user_id) \
                       AS reporter_email, \
                   created_at",
    )
    .bind(user_id)
    .bind(kind)
    .bind(message)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    tracing::info!(
        user_id = user_id,
        report_id = view.id,
        kind = kind.as_str(),
        "feedback report submitted"
    );
    Ok(view)
}

/// Every stored report, newest first — the admin page's list.
///
/// # Errors
/// `AppError::Sqlx` on DB failure.
pub async fn list_all(pool: &SqlitePool) -> Result<Vec<FeedbackReportView>, AppError> {
    let views = sqlx::query_as::<_, FeedbackReportView>(
        "SELECT id, kind, message, \
                (SELECT email FROM users WHERE id = feedback_reports.user_id) \
                    AS reporter_email, \
                created_at \
         FROM feedback_reports \
         ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(views)
}

/// Emails of every admin account — the recipients of the new-report email.
///
/// # Errors
/// `AppError::Sqlx` on DB failure.
pub async fn admin_emails(pool: &SqlitePool) -> Result<Vec<String>, AppError> {
    let emails = sqlx::query_scalar("SELECT email FROM users WHERE role = 'admin' ORDER BY id")
        .fetch_all(pool)
        .await?;
    Ok(emails)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_valid` mirrors the frontend bounds: trimmed-non-empty message of at
    /// most [`MAX_MESSAGE_CHARS`] chars.
    #[test]
    fn input_rejects_blank_and_oversized_messages() {
        assert!(!FeedbackInput::Bug {
            message: "   ".to_string()
        }
        .is_valid());
        assert!(!FeedbackInput::Bug {
            message: String::new()
        }
        .is_valid());
        assert!(!FeedbackInput::Bug {
            message: "x".repeat(MAX_MESSAGE_CHARS + 1)
        }
        .is_valid());
        assert!(FeedbackInput::Bug {
            message: "x".repeat(MAX_MESSAGE_CHARS)
        }
        .is_valid());
        assert!(FeedbackInput::Suggestion {
            message: " add a dark mode ".to_string()
        }
        .is_valid());
    }
}
