//! Payload of the feedback-report alert sent to the admins (a bug report or
//! suggestion submitted from the `/feedback` page).

use super::escape_html;

/// Content of one feedback report notification sent to the admins: an active
/// user submitted a bug report or a suggestion from the `/feedback` page.
#[derive(Debug, Clone)]
pub struct ReportAlert {
    /// Account email of the reporter (the admin follows up with them).
    pub reporter_email: String,
    /// Feedback kind as stored (`"bug"` / `"suggestion"`).
    pub kind: String,
    /// The user's free-form message, verbatim.
    pub message: String,
}

impl ReportAlert {
    /// Human label of the feedback kind ("Bug report" / "Suggestion"), used in
    /// the subject and the email body; an unknown kind renders verbatim.
    #[must_use]
    pub fn kind_label(&self) -> &str {
        match self.kind.as_str() {
            "bug" => "Bug report",
            "suggestion" => "Suggestion",
            other => other,
        }
    }

    /// Email subject ("New bug report from {email}").
    #[must_use]
    pub fn subject(&self) -> String {
        format!(
            "New {} from {}",
            self.kind_label().to_lowercase(),
            self.reporter_email
        )
    }

    /// Plain-text body shared by the email text part (the HTML body is the
    /// `report_alert.html` template).
    #[must_use]
    pub fn plain_text(&self) -> String {
        format!(
            "New {} from {}:\n\n{}",
            self.kind_label().to_lowercase(),
            self.reporter_email,
            self.message
        )
    }

    /// The message as HTML for the template: escaped (free-form user text)
    /// with newlines turned into `<br>`.
    #[must_use]
    pub fn message_html(&self) -> String {
        escape_html(&self.message).replace('\n', "<br>")
    }
}
