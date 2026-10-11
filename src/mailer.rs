use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("resend api error: {0}")]
    Api(String),
}

#[derive(Debug, Clone)]
pub struct MatchAlert {
    pub team: String,
    pub opponent: String,
    pub kickoff: String,
}

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send_welcome(&self, email: &str) -> Result<(), MailError>;
    async fn send_match_alert(&self, email: &str, alert: &MatchAlert) -> Result<(), MailError>;
}

const WELCOME_SUBJECT: &str = "Welcome to OvalBot";

const WELCOME_TEXT: &str = "Your OvalBot account is now active. \
You can now log in and view the Stade Rochelais home matches.";

const DEFAULT_WELCOME_HTML: &str = "<p>Your OvalBot account is now active.</p>\
<p>You can now log in and view the Stade Rochelais home matches.</p>";

const DEFAULT_MATCH_ALERT_HTML: &str =
    "<p>Good news! <strong>{{team}} vs {{opponent}}</strong> has been scheduled for {{kickoff}}.</p>";

#[derive(Debug, Clone)]
pub struct EmailTemplates {
    pub welcome_html: String,
    pub match_alert_html: String,
}

impl EmailTemplates {
    pub fn defaults() -> Self {
        Self {
            welcome_html: DEFAULT_WELCOME_HTML.to_string(),
            match_alert_html: DEFAULT_MATCH_ALERT_HTML.to_string(),
        }
    }
}

pub fn load_templates(dir: &Path) -> EmailTemplates {
    let mut templates = EmailTemplates::defaults();
    if let Ok(welcome) = std::fs::read_to_string(dir.join("welcome.html")) {
        templates.welcome_html = welcome;
    } else {
        tracing::warn!(
            "welcome.html template not found in {}, using default",
            dir.display()
        );
    }
    if let Ok(match_alert) = std::fs::read_to_string(dir.join("match_alert.html")) {
        templates.match_alert_html = match_alert;
    } else {
        tracing::warn!(
            "match_alert.html template not found in {}, using default",
            dir.display()
        );
    }
    templates
}

pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

#[derive(Serialize)]
struct OutgoingEmail<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    html: &'a str,
    text: &'a str,
}

#[derive(Clone)]
pub struct ResendMailer {
    client: reqwest::Client,
    api_key: String,
    from: String,
    templates: EmailTemplates,
}

impl ResendMailer {
    pub fn new(api_key: String, from: String, templates: EmailTemplates) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            from,
            templates,
        }
    }

    async fn send(
        &self,
        email: &str,
        subject: &str,
        html: &str,
        text: &str,
    ) -> Result<(), MailError> {
        let payload = OutgoingEmail {
            from: &self.from,
            to: email,
            subject,
            html,
            text,
        };
        let response = self
            .client
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(MailError::Api(response.status().to_string()));
        }
        Ok(())
    }
}

#[async_trait]
impl Mailer for ResendMailer {
    async fn send_welcome(&self, email: &str) -> Result<(), MailError> {
        let html = render(&self.templates.welcome_html, &[]);
        self.send(email, WELCOME_SUBJECT, &html, WELCOME_TEXT).await
    }

    async fn send_match_alert(&self, email: &str, alert: &MatchAlert) -> Result<(), MailError> {
        let subject = format!("Match scheduled: {} vs {}", alert.team, alert.opponent);
        let text = format!(
            "Good news! {} vs {} has been scheduled for {}.",
            alert.team, alert.opponent, alert.kickoff
        );
        let html = render(
            &self.templates.match_alert_html,
            &[
                ("team", &alert.team),
                ("opponent", &alert.opponent),
                ("kickoff", &alert.kickoff),
            ],
        );
        self.send(email, &subject, &html, &text).await
    }
}

pub struct NoopMailer;

#[async_trait]
impl Mailer for NoopMailer {
    async fn send_welcome(&self, _email: &str) -> Result<(), MailError> {
        Ok(())
    }

    async fn send_match_alert(&self, _email: &str, _alert: &MatchAlert) -> Result<(), MailError> {
        Ok(())
    }
}

pub fn from_env() -> Arc<dyn Mailer> {
    let enabled = std::env::var("RESEND_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !enabled {
        return Arc::new(NoopMailer);
    }
    let api_key = std::env::var("RESEND_API_KEY").unwrap_or_default();
    let from = std::env::var("RESEND_FROM")
        .unwrap_or_else(|_| "OvalBot <onboarding@resend.dev>".to_string());
    let template_dir = std::env::var("TEMPLATE_DIR").unwrap_or_else(|_| "templates".to_string());
    let templates = load_templates(Path::new(&template_dir));
    Arc::new(ResendMailer::new(api_key, from, templates))
}

pub async fn send_welcome_if_unsent(
    mailer: &dyn Mailer,
    pool: &SqlitePool,
    user_id: i64,
    email: &str,
) {
    let already_sent = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE id = ? AND welcome_email_sent_at IS NOT NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    {
        Ok(count) => count > 0,
        Err(e) => {
            tracing::warn!("failed to check welcome email status for user {user_id}: {e}");
            return;
        }
    };

    if already_sent {
        return;
    }

    match mailer.send_welcome(email).await {
        Ok(()) => {
            let now = crate::time::now();
            if let Err(e) = sqlx::query("UPDATE users SET welcome_email_sent_at = ? WHERE id = ?")
                .bind(&now)
                .bind(user_id)
                .execute(pool)
                .await
            {
                tracing::warn!("failed to record welcome email for user {user_id}: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("failed to send welcome email to {email}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_replaces_known_placeholders() {
        let template = "<p>{{team}} vs {{opponent}} at {{kickoff}}</p>";
        let rendered = render(
            template,
            &[
                ("team", "Stade Rochelais"),
                ("opponent", "RC Toulon"),
                ("kickoff", "samedi 14h30"),
            ],
        );
        assert_eq!(
            rendered,
            "<p>Stade Rochelais vs RC Toulon at samedi 14h30</p>"
        );
    }

    #[test]
    fn render_leaves_unknown_placeholders_untouched() {
        let template = "<p>{{team}} vs {{unknown}}</p>";
        let rendered = render(template, &[("team", "Stade Rochelais")]);
        assert_eq!(rendered, "<p>Stade Rochelais vs {{unknown}}</p>");
    }

    #[test]
    fn load_templates_falls_back_to_defaults_when_dir_missing() {
        let templates = load_templates(Path::new("/nonexistent/ovalbot/templates"));
        assert_eq!(templates.welcome_html, DEFAULT_WELCOME_HTML);
        assert_eq!(templates.match_alert_html, DEFAULT_MATCH_ALERT_HTML);
    }

    #[test]
    fn load_templates_reads_files_from_dir() {
        let dir = std::env::temp_dir().join("ovalbot-mailer-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("welcome.html"), "<h1>Hi</h1>").unwrap();
        std::fs::write(dir.join("match_alert.html"), "<p>{{kickoff}}</p>").unwrap();

        let templates = load_templates(&dir);
        assert_eq!(templates.welcome_html, "<h1>Hi</h1>");
        assert_eq!(templates.match_alert_html, "<p>{{kickoff}}</p>");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
