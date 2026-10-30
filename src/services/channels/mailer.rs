//! Email transport behind the [`Mailer`] trait: the Resend API client, a
//! no-op for disabled deployments, and the welcome/match-alert/seat-alert
//! templates. The alert payloads themselves (channel-neutral) live in
//! [`crate::services::channels::alerts`].
//!
//! Templates live in `TEMPLATE_DIR` (`welcome.html`, `match_alert.html`,
//! `seats_alert.html`); missing files fall back to built-in defaults.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::services::channels::alerts::{CartAlert, MatchAlert, SeatAlert};

/// Failures while sending through Resend: network/serialization issues
/// (`Http`) and non-2xx API responses (`Api`).
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("resend api error: {0}")]
    Api(String),
}

/// Email transport abstraction: injected into [`crate::services::state::AppState`]
/// (`ResendMailer` in production, [`NoopMailer`] when disabled) so tests and
/// offline runs never hit the network.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Sends the account-activation welcome email.
    ///
    /// # Errors
    /// [`MailError`] on network or Resend API failure.
    async fn send_welcome(&self, email: &str) -> Result<(), MailError>;

    /// Sends a match-scheduling alert for one (team, opponent, kickoff) tuple.
    ///
    /// # Errors
    /// [`MailError`] on network or Resend API failure.
    async fn send_match_alert(&self, email: &str, alert: &MatchAlert) -> Result<(), MailError>;

    /// Sends a seats-on-resale alert (new seats for a match the user requested
    /// places for).
    ///
    /// # Errors
    /// [`MailError`] on network or Resend API failure.
    async fn send_seats_alert(&self, email: &str, alert: &SeatAlert) -> Result<(), MailError>;

    /// Sends a "seats are in your cart" notification (the Phase 4B bot added
    /// seats to the user's billetterie cart; pay within ~20 minutes).
    ///
    /// # Errors
    /// [`MailError`] on network or Resend API failure.
    async fn send_cart_alert(&self, email: &str, alert: &CartAlert) -> Result<(), MailError>;
}

const WELCOME_SUBJECT: &str = "Welcome to OvalBot";

const WELCOME_TEXT: &str = "Your OvalBot account is now active. \
You can now log in and view the Stade Rochelais home matches.";

const DEFAULT_WELCOME_HTML: &str = "<p>Your OvalBot account is now active.</p>\
<p>You can now log in and view the Stade Rochelais home matches.</p>";

const DEFAULT_MATCH_ALERT_HTML: &str =
    "<p>Good news! <strong>{{team}} vs {{opponent}}</strong> has been scheduled for {{kickoff}}.</p>";

const DEFAULT_SEATS_ALERT_HTML: &str = "<p>Nouvelles places de rugby en revente pour \
<strong>Stade Rochelais vs {{opponent}}</strong> ({{date}}) :</p>\
<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\">{{seats_html}}\
</table><p><a href=\"{{link}}\">Réserver mes places à la billetterie</a></p>";

const DEFAULT_CART_ALERT_HTML: &str = "<p><strong>{{added}}</strong> place(s) ajoutée(s) à ton \
panier pour Stade Rochelais vs <strong>{{opponent}}</strong> ({{date}}) — {{progress}} au total.</p>\
<p>Tu as environ 20 minutes pour payer.</p><p><a href=\"{{link}}\">Accéder à mon panier</a></p>";

/// The two HTML bodies, loaded from `TEMPLATE_DIR` (see [`load_templates`]).
#[derive(Debug, Clone)]
pub struct EmailTemplates {
    /// Account-activation email body (default variant needs no placeholder).
    pub welcome_html: String,
    /// Match-scheduling alert body with `{{team}}` / `{{opponent}}` /
    /// `{{kickoff}}` placeholders.
    pub match_alert_html: String,
    /// Seats-on-resale alert body with `{{opponent}}` / `{{date}}` /
    /// `{{seats_html}}` / `{{link}}` placeholders (`seats_html` carries the
    /// styled seat rows built by [`SeatAlert::seats_html`]).
    pub seats_alert_html: String,
    /// Cart-ready alert body with `{{opponent}}` / `{{date}}` / `{{added}}` /
    /// `{{progress}}` / `{{link}}` placeholders (the link points at the shop
    /// basket).
    pub cart_alert_html: String,
}

impl EmailTemplates {
    /// The built-in default bodies used when template files are missing.
    pub fn defaults() -> Self {
        Self {
            welcome_html: DEFAULT_WELCOME_HTML.to_string(),
            match_alert_html: DEFAULT_MATCH_ALERT_HTML.to_string(),
            seats_alert_html: DEFAULT_SEATS_ALERT_HTML.to_string(),
            cart_alert_html: DEFAULT_CART_ALERT_HTML.to_string(),
        }
    }
}

/// Loads `welcome.html`, `match_alert.html` and `seats_alert.html` from `dir`.
///
/// # Returns
/// The two templates, falling back to [`EmailTemplates::defaults`] per file
/// (a `warning` is logged for each missing file, never a hard error).
///
/// # Panics
/// None.
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
    if let Ok(seats_alert) = std::fs::read_to_string(dir.join("seats_alert.html")) {
        templates.seats_alert_html = seats_alert;
    } else {
        tracing::warn!(
            "seats_alert.html template not found in {}, using default",
            dir.display()
        );
    }
    if let Ok(cart_alert) = std::fs::read_to_string(dir.join("cart_ready.html")) {
        templates.cart_alert_html = cart_alert;
    } else {
        tracing::warn!(
            "cart_ready.html template not found in {}, using default",
            dir.display()
        );
    }
    templates
}

/// Substitutes `{{key}}` placeholders in a template with `vars` values, in
/// argument order (a later value overwrites an earlier duplicate key);
/// unknown placeholders are left verbatim.
///
/// # Arguments
/// * `template` — HTML/straight text with `{{key}}` slots.
/// * `vars` — `(key, value)` pairs; values are inserted unescaped.
///
/// # Returns
/// The rendered string.
#[must_use]
pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

/// Resend API request body for `POST /emails`.
#[derive(Serialize)]
struct OutgoingEmail<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    html: &'a str,
    text: &'a str,
}

const RESEND_API_URL: &str = "https://api.resend.com";

/// Production [`Mailer`]: sends through the Resend HTTP API with a bearer
/// token (`base_url` is injectable for tests — see `tests/mailer.rs`).
#[derive(Clone)]
pub struct ResendMailer {
    client: reqwest::Client,
    api_key: String,
    from: String,
    templates: EmailTemplates,
    base_url: String,
}

impl ResendMailer {
    /// Creates a mailer targeting the public Resend API; see
    /// [`Self::with_base_url`] to redirect.
    pub fn new(api_key: String, from: String, templates: EmailTemplates) -> Self {
        Self::with_base_url(api_key, from, templates, RESEND_API_URL.to_string())
    }

    /// Same as [`Self::new`] but against a custom `base_url` — the escape
    /// hatch used by transport tests (in-process mock) and by staging.
    pub fn with_base_url(
        api_key: String,
        from: String,
        templates: EmailTemplates,
        base_url: String,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            from,
            templates,
            base_url,
        }
    }

    /// POSTs one email to `POST {base_url}/emails` with the bearer token.
    ///
    /// Shared plumbing of [`Mailer::send_welcome`] /
    /// [`Mailer::send_match_alert`]; the caller renders subject/body.
    ///
    /// # Errors
    /// `MailError::Http` on network failure, `MailError::Api` on any non-2xx
    /// response (status text is carried in the error).
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
        // send thee email to resend and await the response
        let response = self
            .client
            .post(format!("{}/emails", self.base_url))
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
    // send the welcome email
    /// The default welcome subject/body, no placeholder to render.
    async fn send_welcome(&self, email: &str) -> Result<(), MailError> {
        let html = render(&self.templates.welcome_html, &[]);
        self.send(email, WELCOME_SUBJECT, &html, WELCOME_TEXT).await
    }

    // send the match alert email
    /// Subject and text are built from the alert; the rendered HTML template
    /// fills `{{team}}` / `{{opponent}}` / `{{kickoff}}`.
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

    // send the seats alert email
    /// Subject and text are built from the alert; the rendered HTML template
    /// fills `{{opponent}}` / `{{date}}` / `{{seats_html}}` / `{{link}}`
    /// (an empty `link` falls back to `#` on the CTA).
    async fn send_seats_alert(&self, email: &str, alert: &SeatAlert) -> Result<(), MailError> {
        let subject = format!(
            "Nouvelles places en revente : Stade Rochelais vs {}",
            alert.opponent
        );
        let text = format!(
            "Nouvelles places de rugby en revente pour Stade Rochelais vs {} ({}):\n\
             {}\nRéserve vite : {}",
            alert.opponent,
            alert.date,
            alert.seats_listing(),
            alert.link
        );
        // The CTA needs somewhere to go even when the encounter carried no link.
        let link = if alert.link.is_empty() {
            "#"
        } else {
            &alert.link
        };
        let html = render(
            &self.templates.seats_alert_html,
            &[
                ("opponent", &alert.opponent),
                ("date", &alert.date),
                ("seats_html", &alert.seats_html()),
                ("link", link),
            ],
        );
        self.send(email, &subject, &html, &text).await
    }

    /// Notification that seats are in the user's shop cart: subject/text
    /// from the alert, the HTML template fills `{{opponent}}` / `{{date}}` /
    /// `{{added}}` / `{{progress}}` / `{{link}}` (empty link → `#`).
    async fn send_cart_alert(&self, email: &str, alert: &CartAlert) -> Result<(), MailError> {
        let subject = if alert.failed {
            format!(
                "Ajout au panier interrompu : {}/{} places — Stade Rochelais vs {}",
                alert.added, alert.total, alert.opponent
            )
        } else {
            format!(
                "{} place(s) au panier — Stade Rochelais vs {}",
                alert.added, alert.opponent
            )
        };
        let link = if alert.cart_link.is_empty() {
            "#"
        } else {
            &alert.cart_link
        };
        let html = render(
            &self.templates.cart_alert_html,
            &[
                ("opponent", &alert.opponent),
                ("date", &alert.date),
                ("added", &alert.added.to_string()),
                ("progress", &alert.progress()),
                ("link", link),
            ],
        );
        self.send(email, &subject, &html, &alert.plain_text()).await
    }
}

/// The always-succeeding [`Mailer`] used when `RESEND_ENABLED` is not `true`:
/// every send is dropped (no-op).
pub struct NoopMailer;

#[async_trait]
impl Mailer for NoopMailer {
    /// Does nothing, always `Ok(())`.
    async fn send_welcome(&self, _email: &str) -> Result<(), MailError> {
        Ok(())
    }

    /// Does nothing, always `Ok(())`.
    async fn send_match_alert(&self, _email: &str, _alert: &MatchAlert) -> Result<(), MailError> {
        Ok(())
    }

    /// Does nothing, always `Ok(())`.
    async fn send_seats_alert(&self, _email: &str, _alert: &SeatAlert) -> Result<(), MailError> {
        Ok(())
    }

    /// Does nothing, always `Ok(())`.
    async fn send_cart_alert(&self, _email: &str, _alert: &CartAlert) -> Result<(), MailError> {
        Ok(())
    }
}

/// Builds the mailer from the environment:
/// RESEND_ENABLED=false → [`NoopMailer`]; otherwise a [`ResendMailer`] with
/// RESEND_API_KEY, RESEND_FROM and the TEMPLATE_DIR templates.
///
/// # Panics
/// None (missing api key yields an empty-token mailer; sending will fail at
/// the API level rather than here).
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

/// Sends the welcome email to a freshly active user at most once.
///
/// Best-effort by design: every outcome other than a successful send is
/// logged and dropped, never returned. The `welcome_email_sent_at` column (set right after
/// a successful send) makes the send idempotent across repeated calls, so
/// direct registrations, auto-promotions and manual activations can all call
/// this blindly.
///
/// # Arguments
/// * `user_id` — the row the anti-double-send marker is read/written on.
///
/// # Panics
/// None.
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
            let now = crate::common::time::now();
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
    use crate::services::channels::alerts::SeatBrief;

    /// `render` replaces known `{{placeholders}}` with the provided values.
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

    /// `render` leaves unknown placeholders untouched.
    #[test]
    fn render_leaves_unknown_placeholders_untouched() {
        let template = "<p>{{team}} vs {{unknown}}</p>";
        let rendered = render(template, &[("team", "Stade Rochelais")]);
        assert_eq!(rendered, "<p>Stade Rochelais vs {{unknown}}</p>");
    }

    /// `load_templates` falls back to the built-in defaults when the directory is
    /// missing.
    #[test]
    fn load_templates_falls_back_to_defaults_when_dir_missing() {
        let templates = load_templates(Path::new("/nonexistent/ovalbot/templates"));
        assert_eq!(templates.welcome_html, DEFAULT_WELCOME_HTML);
        assert_eq!(templates.match_alert_html, DEFAULT_MATCH_ALERT_HTML);
    }

    /// `load_templates` reads `welcome.html` and `match_alert.html` from a directory.
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

    /// `NoopMailer` returns success for every send without doing anything.
    #[tokio::test]
    async fn noop_mailer_sends_nothing() {
        let mailer = NoopMailer;
        assert!(mailer.send_welcome("a@example.com").await.is_ok());
        let alert = MatchAlert {
            team: "Stade Rochelais".to_string(),
            opponent: "RC Toulon".to_string(),
            kickoff: "samedi 14h30".to_string(),
        };
        assert!(mailer
            .send_match_alert("a@example.com", &alert)
            .await
            .is_ok());
        let seats = SeatAlert {
            opponent: "RC Toulon".to_string(),
            date: "dimanche 20 décembre à 15h15".to_string(),
            link: String::new(),
            seats: vec![SeatBrief {
                bloc: "1".to_string(),
                row: "1".to_string(),
                seat_number: 1,
                price: "49.00 €".to_string(),
                full_name: "Rang 1 • Siège 1".to_string(),
            }],
        };
        assert!(mailer
            .send_seats_alert("a@example.com", &seats)
            .await
            .is_ok());
    }

    /// `ResendMailer::new` targets the public Resend API by default.
    #[test]
    fn new_uses_default_base_url() {
        let mailer = ResendMailer::new("key".into(), "from".into(), EmailTemplates::defaults());
        assert_eq!(mailer.base_url, RESEND_API_URL);
    }
}
