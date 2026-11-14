//! Channel-neutral alert payloads shared by every notification channel
//! (email via the [`crate::services::channels::mailer`] transport, Telegram
//! text via [`SeatAlert::telegram_text`] and sent with
//! [`crate::services::channels::telegram`]). Pure data + display formatting —
//! no transport, no persistence.
//!
//! One sub-module per payload: `scheduling` (match kickoff), `seats`
//! (resale seats), `cart` (seats added to the shop cart) and `report`
//! (feedback reports to the admins); the shared Telegram helpers live here.

mod cart;
mod report;
mod scheduling;
mod seats;

pub use cart::CartAlert;
pub use report::ReportAlert;
pub use scheduling::MatchAlert;
pub use seats::{SeatAlert, SeatBrief};

/// Horizontal rule of the Telegram alert messages (16 heavy dashes), exactly
/// as the user-validated design.
pub(crate) const DIVIDER: &str = "━━━━━━━━━━━━━━━━";

/// Escapes the characters that would break the Telegram `parse_mode: "HTML"`
/// message: `&`, `<`, `>`, and `"` (harmless in text, required in the CTA's
/// `href` attribute). Applied to every free-form site field before
/// interpolation.
pub(crate) fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
