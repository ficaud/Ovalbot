//! Channel-neutral alert payloads shared by every notification channel
//! (email via the [`crate::services::channels::mailer`] transport, Telegram text
//! via [`SeatAlert::telegram_text`] and sent with
//! [`crate::services::channels::telegram`]). Pure data + display formatting —
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
    /// The site's own seat text, ` • `-separated ("Tribune PELOUSE • Bloc
    /// PELOUSE • Accès PELOUSE • Siège 475"); the Telegram message renders it
    /// verbatim instead of recomposing it from the fields above.
    pub full_name: String,
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

    /// Styled HTML rows for the seats-alert email body: one full-width row
    /// per seat — bold seat line (bloc + price, columnless text flow) and a
    /// gray position line ("Rang U · Siège 2") underneath — designed to be
    /// injected inside the template's fixed-cell seats table. See the private
    /// `seat_html_row` for why the layout must stay columnless.
    #[must_use]
    pub fn seats_html(&self) -> String {
        self.seats
            .iter()
            .map(seat_html_row)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Telegram seats-alert message, an HTML fragment to send with
    /// `parse_mode: "HTML"`: header, divider, opponent + date block, the
    /// clickable resale CTA, then one `  • {full_name} — {price}€` bullet per
    /// seat — the seat line keeps the site's own `full_name` text verbatim
    /// (tribune and position exactly as displayed on the billetterie).
    /// Free-form site fields are HTML-escaped before interpolation.
    ///
    /// Degrades cleanly: a missing link drops the CTA line.
    #[must_use]
    pub fn telegram_text(&self) -> String {
        let mut text = format!(
            "🏉 Stade Rochelais\n\n{DIVIDER}\n\n🆚 {}\n📅 {}\n\n🟢 Nouvelles places :\n",
            escape_html(&self.opponent),
            escape_html(&self.date),
        );
        if !self.link.is_empty() {
            text.push_str(&format!(
                "🔗 <a href=\"{}\">Accéder à la revente</a>\n",
                escape_html(&self.link)
            ));
        }
        text.push('\n');
        let lines: Vec<String> = self
            .seats
            .iter()
            .map(|seat| {
                format!(
                    "  • {} — {}€",
                    escape_html(&seat.full_name),
                    escape_html(&seat.price)
                )
            })
            .collect();
        text.push_str(&lines.join("\n"));
        text
    }
}

/// Builds the HTML row for one seat: one full-width cell holding a bold
/// ticket line (`🎟️ {bloc}` in white, `— {price} €` in `#FFD500`, same text
/// flow) and the gray position line ("Rang {row} · Siège {n}") underneath.
///
/// Columnless by design: the earlier 99%/1% two-column split (price cell
/// `width="1%"` + `nowrap`) was honored by Apple Mail as a 1%-wide column
/// with the wrap hints dropped, rendering the price one character per line
/// and misaligning both rows. A single text flow inside one cell has no
/// column to shrink, so it can only wrap at word boundaries — the safe
/// layout for every client. Extension or redesign must not reintroduce
/// multi-column tables here.
///
/// Empty parts are dropped (a pelouse seat has no bloc, some pack data has
/// no position); a seat with nothing left keeps "Place" and the price.
///
/// # Arguments
/// * `seat` — the seat to render.
///
/// # Returns
/// One `<tr>` element.
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
            "<p style=\"margin:2px 0 0;font-size:13px;color:#94a3b8;line-height:1.4;\">{position}</p>"
        )
    };

    format!(
        "<tr><td style=\"padding:12px 0;font-size:15px;color:#ffffff;font-weight:bold;line-height:1.4;\"><p style=\"margin:0;\">🎟️ {label} <span style=\"color:#FFD500;\">— {} €</span></p>{detail}</td></tr>",
        seat.price
    )
}

/// Horizontal rule of the Telegram seats-alert message (16 heavy dashes),
/// exactly as the user-validated design.
const DIVIDER: &str = "━━━━━━━━━━━━━━━━";

/// Escapes the characters that would break the Telegram `parse_mode: "HTML"`
/// message: `&`, `<`, `>`, and `"` (harmless in text, required in the CTA's
/// `href` attribute). Applied to every free-form site field before
/// interpolation.
fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
            full_name: format!("{bloc} • Rang {row} • Siège {seat_number}"),
        }
    }

    /// `seats_html` renders one full-width row per seat: bold ticket line
    /// (bloc + price in the same text flow) and position line underneath.
    #[test]
    fn seats_html_renders_one_flow_row_per_seat() {
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
        assert_eq!(html.matches("<tr>").count(), 2);

        // Columnless layout, asserted dead: any `width="…%"` split or
        // `nowrap` hint can be mis-rendered by mobile clients (Apple Mail
        // stacked the price one character per line on the old design).
        assert!(!html.contains("width=\""));
        assert!(!html.contains("nowrap"));
    }

    // An open-area seat (no bloc) falls back to a generic label; a pack with
    // no position data drops the gray line instead of rendering empty text.
    #[test]
    fn seats_html_tolerates_missing_parts() {
        let alert = SeatAlert {
            opponent: "Aviron Bayonnais".to_string(),
            date: "dimanche 8 novembre à 21h05".to_string(),
            link: String::new(),
            seats: vec![SeatBrief {
                bloc: String::new(),
                row: String::new(),
                seat_number: 0,
                price: "15.00".to_string(),
                full_name: String::new(),
            }],
        };

        let html = alert.seats_html();
        assert!(html.contains("🎟️ Place"));
        assert!(html.contains("15.00 €"));
        assert!(!html.contains("Rang"));
        assert!(!html.contains("Siège"));
        assert_eq!(html.matches("<tr>").count(), 1);
    }

    /// `telegram_text` renders the user-validated design: header, divider,
    /// opponent/date block, clickable CTA, and one bullet per seat keeping
    /// the site's own seat text.
    #[test]
    fn telegram_text_matches_the_house_design() {
        let alert = SeatAlert {
            opponent: "UNION BORDEAUX BÈGLES".to_string(),
            date: "dimanche 25 octobre à 21h05".to_string(),
            link: "https://billetterie.staderochelais.com/fr/product/1357/revente".to_string(),
            seats: vec![SeatBrief {
                bloc: String::new(),
                row: String::new(),
                seat_number: 475,
                price: "15.00".to_string(),
                full_name: "Tribune PELOUSE • Bloc PELOUSE • Accès PELOUSE • Siège 475".to_string(),
            }],
        };

        assert_eq!(
            alert.telegram_text(),
            "🏉 Stade Rochelais\n\n\
             ━━━━━━━━━━━━━━━━\n\n\
             🆚 UNION BORDEAUX BÈGLES\n\
             📅 dimanche 25 octobre à 21h05\n\n\
             🟢 Nouvelles places :\n\
             🔗 <a href=\"https://billetterie.staderochelais.com/fr/product/1357/revente\">Accéder à la revente</a>\n\n\
             \x20 • Tribune PELOUSE • Bloc PELOUSE • Accès PELOUSE • Siège 475 — 15.00€"
        );
    }

    // Multiple seats join as one bullet per line; a missing link drops the
    // CTA line.
    #[test]
    fn telegram_text_tolerates_missing_parts() {
        let alert = SeatAlert {
            opponent: "Aviron Bayonnais".to_string(),
            date: "dimanche 8 novembre à 21h05".to_string(),
            link: String::new(),
            seats: vec![seat("NORD", "U", 2, "49.00"), seat("NORD", "U", 3, "49.00")],
        };

        let text = alert.telegram_text();
        assert!(!text.contains("🔗"));
        assert!(text.contains(
            "  • NORD • Rang U • Siège 2 — 49.00€\n  • NORD • Rang U • Siège 3 — 49.00€"
        ));
    }

    // Free-form site fields are HTML-escaped: the fragment stays a valid
    // parse_mode="HTML" document even with angle brackets or ampersands.
    #[test]
    fn telegram_text_escapes_free_form_fields() {
        let alert = SeatAlert {
            opponent: "A&B <Club>".to_string(),
            date: "dimanche 8 novembre".to_string(),
            link: "https://example.com/search?q=rugby&sort=price".to_string(),
            seats: vec![SeatBrief {
                bloc: String::new(),
                row: String::new(),
                seat_number: 1,
                price: "10.00".to_string(),
                full_name: "<span>Rang & Siège</span>".to_string(),
            }],
        };

        let text = alert.telegram_text();
        assert!(text.contains("🆚 A&amp;B &lt;Club&gt;"));
        assert!(text.contains("href=\"https://example.com/search?q=rugby&amp;sort=price\""));
        assert!(text.contains("&lt;span&gt;Rang &amp; Siège&lt;/span&gt;"));
        assert!(!text.contains("<span>"));
        assert!(!text.contains("A&B <Club>"));
    }
}
