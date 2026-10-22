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
    /// Bloc where the seat sits in the stadium.
    pub bloc: String,
    /// Row letter.
    pub row: String,
    /// Seat number.
    pub seat_number: u64,
    /// Raw price string from the site.
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
    /// The new seats, in scan order.
    pub seats: Vec<SeatBrief>,
}

impl SeatAlert {
    /// Compact one-line listing of the new seats, shared by the email body
    /// and the Telegram message ("Bloc MACIF NORD - Rang U - Siège 19 for 49.00 €").
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
}
