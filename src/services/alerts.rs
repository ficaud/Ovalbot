//! Channel-neutral alert payloads shared by every notification channel
//! (email via the [`crate::services::mailer`] transport, Telegram text built
//! in [`crate::services::notifications`]). Pure data + display formatting —
//! no transport, no persistence.

/// Content of one match-scheduling alert, shared by every channel.
#[derive(Debug, Clone)]
pub struct MatchAlert {
    pub team: String,
    pub opponent: String,
    pub kickoff: String,
}

/// One resale seat as displayed to the user: stadium location and price
/// exactly as seen on the billetterie ("Bloc MACIF NORD", "49.00 €").
#[derive(Debug, Clone)]
pub struct SeatBrief {
    /// Bloc where the seat sits in the stadium (empty for open areas like
    /// "Pelouse" that have no bounded bloc).
    pub bloc: String,
    /// Row letter or number, without the "Rang" prefix.
    pub row: String,
    /// Seat number.
    pub seat_number: u64,
    /// Raw price string from the site, without the `€` suffix.
    pub price: String,
}

/// Content of one seats-on-resale alert, shared by every channel: a resolved
/// encounter whose resale gained seats the user never heard of before.
#[derive(Debug, Clone)]
pub struct SeatAlert {
    /// Encounter title from the billetterie (the opponent's display name).
    pub opponent: String,
    /// Encounter date, free-form French string from the site.
    pub date: String,
    /// Link to the resale product page on the billetterie; empty when unknown
    /// (the channels fall back to `#` or omit the link).
    pub link: String,
    /// The new seats, in scan order.
    pub seats: Vec<SeatBrief>,
}

impl SeatAlert {
    /// Compact one-line listing of the new seats, shared by the email plain
    /// text and the Telegram message ("Bloc MACIF NORD - Rang U - Siège 19
    /// for 49.00 €").
    #[must_use]
    pub fn seats_listing(&self) -> String {
        self.seats
            .iter()
            .map(|seat| {
                format!(
                    "{} - {} - {} for {} €",
                    seat.bloc, seat.row, seat.seat_number, seat.price
                )
            })
            .collect::<Vec<_>>()
            .join(",\n\n")
    }

    /// Styled HTML table rows for the seats-alert email body: one row pair
    /// per seat — bold seat label (bloc) with the price on the right in
    /// `#FFD500`, gray position line ("Rang U · Siège 2") underneath —
    /// designed to be injected inside the template's fixed-cell seats table.
    #[must_use]
    pub fn seats_html(&self) -> String {
        self.seats
            .iter()
            .map(seat_html_row)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Builds the HTML rows for one seat: a bold `🎟️ {bloc}` cell with the price
/// on the right, plus the position line ("Rang {row} · Siège {n}") below.
/// Empty parts are dropped (a pelouse seat has no bloc, some pack data has no
/// position); a seat with nothing left to show keeps just label and price.
///
/// # Arguments
/// * `seat` — the seat to render.
///
/// # Returns
/// Two `<tr>` elements (or one when the position line is empty).
#[must_use]
fn seat_html_row(seat: &SeatBrief) -> String {
    let mut position_parts = Vec::new();
    if !seat.row.is_empty() {
        position_parts.push(format!("Rang {}", seat.row));
    }
    if seat.seat_number > 0 {
        position_parts.push(format!("Siège {}", seat.seat_number));
    }
    let position = position_parts.join(" · ");

    let label = if seat.bloc.is_empty() {
        "Place".to_string()
    } else {
        seat.bloc.clone()
    };

    let detail = if position.is_empty() {
        String::new()
    } else {
        format!(
            "<tr><td colspan=\"2\" style=\"padding-bottom:8px;font-size:13px;color:#94a3b8;\">{position}</td></tr>"
        )
    };

    format!(
        "<tr><td style=\"font-size:15px;color:#ffffff;font-weight:bold;\">🎟️ {label}</td>\
<td align=\"right\" style=\"font-size:16px;color:#FFD500;font-weight:bold;white-space:nowrap;\">{} €</td></tr>{detail}",
        seat.price
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The default fixture seat: bounded bloc, full position.
    fn seat(bloc: &str, row: &str, seat_number: u64, price: &str) -> SeatBrief {
        SeatBrief {
            bloc: bloc.to_string(),
            row: row.to_string(),
            seat_number,
            price: price.to_string(),
        }
    }

    // One row pair per seat: bold label + price on the right + position line.
    #[test]
    fn seats_html_renders_one_row_pair_per_seat() {
        let alert = SeatAlert {
            opponent: "Aviron Bayonnais".to_string(),
            date: "dimanche 8 novembre à 21h05".to_string(),
            link: String::new(),
            seats: vec![
                seat("NORD - MACIF BRP", "U", 2, "49.00"),
                seat("Pelouse ouest", "5", 12, "15.00"),
            ],
        };

        let html = alert.seats_html();
        assert!(html.contains("🎟️ NORD - MACIF BRP"));
        assert!(html.contains("Rang U · Siège 2"));
        assert!(html.contains("49.00 €"));
        assert!(html.contains("🎟️ Pelouse ouest"));
        assert!(html.contains("Rang 5 · Siège 12"));
        assert!(html.contains("15.00 €"));
        assert_eq!(html.matches("<tr>").count(), 4);
    }

    // An open-area seat (no bloc) falls back to a generic label; a pack with
    // no position data drops the gray line instead of rendering empty text.
    #[test]
    fn seats_html_tolerates_missing_parts() {
        let alert = SeatAlert {
            opponent: "Aviron Bayonnais".to_string(),
            date: "dimanche 8 novembre à 21h05".to_string(),
            link: String::new(),
            seats: vec![seat("", "", 0, "15.00")],
        };

        let html = alert.seats_html();
        assert!(html.contains("🎟️ Place"));
        assert!(html.contains("15.00 €"));
        assert!(!html.contains("Rang"));
        assert!(!html.contains("Siège"));
        assert_eq!(html.matches("<tr>").count(), 1);
    }
}
