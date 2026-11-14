//! Payload of the cart alert ("seats are in your cart, go pay") and its
//! per-channel renderings (email plain text, Telegram message).

use super::{escape_html, DIVIDER};

/// Content of one "seats are in your cart" notification, shared by every
/// channel.
#[derive(Debug, Clone)]
pub struct CartAlert {
    /// Encounter title from the billetterie (the opponent's display name).
    pub opponent: String,
    /// Encounter date, free-form French string from the site.
    pub date: String,
    /// The match's ticketing page (the resale product the seats came from) —
    /// the primary call to action of the notification.
    pub match_link: String,
    /// The shop cart page where the added seats wait to be paid.
    pub cart_link: String,
    /// Seats this wave put into the cart (the delta of one checkout run).
    pub added: usize,
    /// Seats the user holds in the cart for this match **after** the wave: the
    /// cumulative claim count, i.e. what the completion check reads. This is
    /// the number the `claimed/total` progress is built from, so a progressive
    /// order always shows the cart content, never just the last wave.
    pub claimed: usize,
    /// Total seats ordered for this match (`quantity` / `places`).
    pub total: u32,
    /// `true` when the wave ended on a hard checkout failure: the cart holds
    /// what it holds, no more will be added automatically.
    pub failed: bool,
}

impl CartAlert {
    /// The `claimed/total` progress line, shared by the channels.
    #[must_use]
    pub fn progress(&self) -> String {
        format!("{}/{}", self.claimed, self.total)
    }

    /// The primary link of the notification: the match's ticketing page, with
    /// the cart as a fallback when the match page is unknown.
    #[must_use]
    fn match_or_cart_link(&self) -> &str {
        if self.match_link.is_empty() {
            &self.cart_link
        } else {
            &self.match_link
        }
    }

    /// Plain-text body shared by the email text part (the HTML body is the
    /// `cart_ready.html` template).
    #[must_use]
    pub fn plain_text(&self) -> String {
        if self.failed {
            format!(
                "L'ajout automatique au panier a échoué après {} place(s) ({} commandée(s)) \
                 pour Stade Rochelais vs {} ({}).\n\
                 Va payer ce qui est déjà dans ton panier, ou recommence la demande : {}\n\
                 Billets du match : {}",
                self.claimed,
                self.total,
                self.opponent,
                self.date,
                self.cart_link,
                self.match_or_cart_link()
            )
        } else {
            format!(
                "{} place(s) ajoutée(s) à ton panier pour Stade Rochelais vs {} ({}) — \
                 {} au total.\n\
                 Voir les billets du match : {}\n\
                 Tu as environ 20 minutes pour payer : {}",
                self.added,
                self.opponent,
                self.date,
                self.progress(),
                self.match_or_cart_link(),
                self.cart_link
            )
        }
    }

    /// Telegram cart notification, an HTML fragment to send with
    /// `parse_mode: "HTML"`, in the house design (header, divider, match
    /// block, progress line, clickable cart CTA, payment-time reminder).
    /// Degrades cleanly: a missing cart link drops the CTA line. Free-form
    /// site fields are HTML-escaped before interpolation.
    #[must_use]
    pub fn telegram_text(&self) -> String {
        let mut text = format!(
            "🏉 Stade Rochelais\n\n{DIVIDER}\n\n🆚 {}\n📅 {}\n\n",
            escape_html(&self.opponent),
            escape_html(&self.date),
        );
        if self.failed {
            text.push_str(&format!(
                "⚠️ Ajout au panier interrompu : {} place(s) dedans ({} commandée(s)).\n",
                self.claimed, self.total
            ));
        } else {
            text.push_str(&format!(
                "🛒 {} place(s) ajoutée(s) au panier — {} au total.\n",
                self.added,
                self.progress()
            ));
        }
        if !self.match_link.is_empty() {
            text.push_str(&format!(
                "🎟️ <a href=\"{}\">Voir les billets du match</a>\n",
                escape_html(&self.match_link)
            ));
        }
        if !self.cart_link.is_empty() {
            text.push_str(&format!(
                "🛒 <a href=\"{}\">Accéder au panier</a>\n",
                escape_html(&self.cart_link)
            ));
        }
        text.push_str("\n⏳ Tu as environ 20 minutes pour payer.");
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cart alert renders the wave progress and the clickable cart CTA in
    // the house design, with the 20-minute reminder.
    #[test]
    fn cart_telegram_text_renders_progress_and_cta() {
        let alert = CartAlert {
            opponent: "AVIRON BAYONNAIS".to_string(),
            date: "dimanche 8 novembre à 21h05".to_string(),
            match_link: "https://billetterie.staderochelais.com/fr/product/1371/revente_j8"
                .to_string(),
            cart_link: "https://billetterie.staderochelais.com/fr/basket".to_string(),
            added: 2,
            claimed: 3,
            total: 4,
            failed: false,
        };

        assert_eq!(
            alert.telegram_text(),
            "🏉 Stade Rochelais\n\n\
             ━━━━━━━━━━━━━━━━\n\n\
             🆚 AVIRON BAYONNAIS\n\
             📅 dimanche 8 novembre à 21h05\n\n\
             🛒 2 place(s) ajoutée(s) au panier — 3/4 au total.\n\
             🎟️ <a href=\"https://billetterie.staderochelais.com/fr/product/1371/revente_j8\">Voir les billets du match</a>\n\
             🛒 <a href=\"https://billetterie.staderochelais.com/fr/basket\">Accéder au panier</a>\n\n\
             ⏳ Tu as environ 20 minutes pour payer."
        );

        // A failed wave says what is in the cart; an empty cart link drops the
        // cart CTA but keeps the match one.
        let failed = CartAlert {
            cart_link: String::new(),
            failed: true,
            ..alert.clone()
        };
        let text = failed.telegram_text();
        assert!(
            text.contains("⚠️ Ajout au panier interrompu : 3 place(s) dedans (4 commandée(s)).")
        );
        assert!(text.contains("Voir les billets du match"));
        assert!(!text.contains("Accéder au panier"));
        assert!(text.contains("⏳ Tu as environ 20 minutes pour payer."));
    }

    // The plain-text body (email text part) spells out the progress too.
    #[test]
    fn cart_plain_text_matches_the_progress() {
        let alert = CartAlert {
            opponent: "AVIRON BAYONNAIS".to_string(),
            date: "dimanche 8 novembre à 21h05".to_string(),
            match_link: "https://billetterie.staderochelais.com/fr/product/1371/revente_j8"
                .to_string(),
            cart_link: "https://billetterie.staderochelais.com/fr/basket".to_string(),
            added: 1,
            claimed: 2,
            total: 4,
            failed: false,
        };
        let text = alert.plain_text();
        assert!(text.contains("1 place(s) ajoutée(s)"));
        assert!(text.contains("2/4"));
        assert!(text.contains(
            "Voir les billets du match : https://billetterie.staderochelais.com/fr/product/1371/revente_j8"
        ));
        assert!(text.contains("https://billetterie.staderochelais.com/fr/basket"));

        let failed = CartAlert {
            failed: true,
            ..alert
        };
        let text = failed.plain_text();
        assert!(text.contains("a échoué après 2 place(s) (4 commandée(s))"));
        assert!(text.contains("Billets du match"));
    }
}
