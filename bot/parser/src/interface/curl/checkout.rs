//! Shop checkout for Stade Rochelais: authenticated login and the Drupal AJAX
//! add-to-cart flow.
//!
//! This module is the only consumer-facing piece of the purchase feature: the
//! OvalBot orchestrator opens a [`ShopSession`] with credentials, then calls
//! [`ShopSession::add_seat`] once per seat inside the same login session. The
//! passive scan never touches it.
//!
//! The flow mirrors the billetterie's own Drupal behavior:
//! - Two-step login (`email` then `password`) against `/fr/user/login`, with a
//!   fresh `form_build_id` extracted between the steps.
//! - Authentication verified on `/fr/basket` (`user-logged-in` marker) — a
//!   failed step does *not* fail the flow, only the basket check tells the
//!   truth.
//! - Add-to-cart is a form-encoded `POST` of `{ajax_url}` from the page's
//!   `drupal-settings-json` (`edit-add-to-cart` entry) with
//!   `_wrapper_format=drupal_ajax`, the CSRF field `form_token` (absent on
//!   anonymous captures — empty is tolerated) and an `ajax_page_state` replay
//!   (`theme` + `libraries`), sent with the same `X-Requested-With` XHR
//!   headers the site's own frontend uses.
//! - A stale/invalid-session rejection is detected by its body markers
//!   ("périmé" / `alert-danger` + `formulaire`) → [`CheckoutError::ExpiredForm`].
//! - Sessions are deliberately short-lived: the cookie jar lives in a temp
//!   file and is deleted when the [`ShopSession`] is dropped.

use crate::app::clubs::larochelle::parse_seat;
use crate::core::seat::SeatId;
use crate::interface::match_manager::is_waiting_queue;
use curl::easy::{Easy, List};
use scraper::{Html, Selector};
use std::path::PathBuf;
use std::time::Duration;

// ============================== CONSTANTS ==============================

/// The billetterie base URL the checkout is wired to.
pub const SHOP_BASE: &str = "https://billetterie.staderochelais.com";

/// The shop theme name — the fallback when the settings JSON lacks it (the
/// live captures always carry it).
const DEFAULT_THEME: &str = "hubber_reference8";

/// Login and basket paths of the ticketing site.
const LOGIN_PATH: &str = "/fr/user/login";
const BASKET_PATH: &str = "/fr/basket";

// ============================== ERRORS ==============================

/// Why one checkout step failed.
///
/// The variants distinguish a caller-actionable outcome
/// ([`SeatGone`](CheckoutError::SeatGone) and
/// [`WaitingQueue`](CheckoutError::WaitingQueue) — retrying later may still
/// work) from a hard failure (login refused, expired form), which ends the
/// attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutError {
    /// A transport-level failure (timeout, connection reset, HTTP ≥ 400).
    Network(String),
    /// The login flow did not reach an authenticated session.
    Login(String),
    /// The fresh resale page did not carry the add-to-cart machinery
    /// (settings JSON, slider or form id missing).
    MissingContext,
    /// The seat is no longer listed on the fresh resale page — someone took it.
    SeatGone,
    /// The add-to-cart POST was rejected as an expired form / invalid session.
    ExpiredForm,
    /// The billetterie served its waiting-queue page instead of the content.
    WaitingQueue,
}

/// One-line spelling of a checkout error, used by the caller's logs.
impl std::fmt::Display for CheckoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckoutError::Network(e) => f.write_str(&format!("network error: {e}")),
            CheckoutError::Login(e) => f.write_str(&format!("shop login failed: {e}")),
            CheckoutError::MissingContext => {
                f.write_str("add-to-cart form not found on the resale page")
            }
            CheckoutError::SeatGone => f.write_str("seat no longer present on the resale page"),
            CheckoutError::ExpiredForm => {
                f.write_str("add-to-cart rejected: expired form or invalid session")
            }
            CheckoutError::WaitingQueue => f.write_str("billetterie waiting queue"),
        }
    }
}

impl std::error::Error for CheckoutError {}

// ============================== ACTION CONTEXT ==============================

/// The add-to-cart machinery extracted from one rendering of a resale page:
/// exactly what the Drupal AJAX POST needs, everything except the seat
/// identity itself.
#[derive(Debug, Clone)]
pub struct ActionContext {
    /// The `edit-add-to-cart` AJAX endpoint, e.g.
    /// `/fr/product/1357/revente_…?ajax_form=1`.
    pub ajax_url: String,
    /// Replayable `ajaxPageState.libraries` payload of the request.
    pub libraries: String,
    /// `ajaxPageState.theme` name.
    pub theme: String,
    /// Low bound of the price slider, as displayed (e.g. `"44.00"`).
    pub price_min: String,
    /// High bound of the price slider, as displayed (e.g. `"59"`).
    pub price_max: String,
    /// Drupal per-render form id (`form_build_id`).
    pub form_build_id: String,
    /// Drupal CSRF token (`form_token`); empty on anonymous captures where the
    /// site does not emit one.
    pub form_token: String,
}

/// Private module containing the JSON structures deserialized from the page's
/// `drupal-settings-json` script.
mod json {
    use serde::Deserialize;
    use std::collections::HashMap;

    /// Root of the `drupal-settings-json` script, reduced to what the
    /// add-to-cart request replays.
    #[derive(Deserialize)]
    pub struct Settings {
        /// The page's replayable AJAX state (libraries, theme).
        #[serde(rename = "ajaxPageState")]
        pub ajax_page_state: Option<AjaxPageState>,
        /// Drupal AJAX actions by form element id.
        pub ajax: HashMap<String, AjaxEntry>,
    }

    /// Replayable `ajaxPageState` fields.
    #[derive(Deserialize)]
    pub struct AjaxPageState {
        /// Libraries string to echo in the POST.
        pub libraries: Option<String>,
        /// The theme name, e.g. `hubber_reference8`.
        pub theme: Option<String>,
    }

    /// One registered Drupal AJAX action.
    #[derive(Deserialize)]
    pub struct AjaxEntry {
        /// The AJAX endpoint used by the action.
        pub url: String,
    }
}

/// Extracts the [`ActionContext`] of a resale page: the `drupal-settings-json`
/// values plus the form ids. `None` when the page misses any required piece
/// (settings, `edit-add-to-cart` url, slider, form id) — e.g. a closed resale
/// page.
///
/// # Arguments
/// * `html` - The resale page HTML
/// # Returns
/// The page context, or `None` when the add-to-cart machinery is absent
pub fn extract_action_context(html: &str) -> Option<ActionContext> {
    let document = Html::parse_document(html);

    let script_selector =
        Selector::parse("script[data-drupal-selector='drupal-settings-json']").unwrap();
    let settings_json = document
        .select(&script_selector)
        .next()?
        .text()
        .collect::<String>();
    let settings: json::Settings = serde_json::from_str(&settings_json).ok()?;

    let ajax_url = settings.ajax.get("edit-add-to-cart")?.url.clone();
    if ajax_url.is_empty() {
        return None;
    }

    let slider_selector = Selector::parse("input.hubber-slider-input").unwrap();
    let slider = document.select(&slider_selector).next()?;
    let price_min = slider.value().attr("data-min").unwrap_or("").to_string();
    let price_max = slider.value().attr("data-max").unwrap_or("").to_string();

    // form_build_id is required (the form must be on the page); form_token is
    // optional — anonymous pages ship without one and the POST still works.
    let form_build_id = extract_form_build_id(html)?;
    let form_token = extract_form_token(html).unwrap_or_default();

    Some(ActionContext {
        ajax_url,
        libraries: settings
            .ajax_page_state
            .as_ref()
            .and_then(|s| s.libraries.clone())
            .unwrap_or_default(),
        theme: settings
            .ajax_page_state
            .as_ref()
            .and_then(|s| s.theme.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| DEFAULT_THEME.to_string()),
        price_min,
        price_max,
        form_build_id,
        form_token,
    })
}

/// Extracts the Drupal `form_build_id` from a form page (plain HTML form or
/// the `\u0022`-escaped variant seen inside JSON payloads).
///
/// # Arguments
/// * `html` - The page HTML
/// # Returns
/// The form build id, or `None` when the page carries none
pub fn extract_form_build_id(html: &str) -> Option<String> {
    extract_hidden_input(html, "form_build_id")
}

/// Extracts the Drupal `form_token` (CSRF) with the same two form variants as
/// [`extract_form_build_id`].
///
/// # Arguments
/// * `html` - The page HTML
/// # Returns
/// The token, or `None` when absent (anonymous pages)
pub fn extract_form_token(html: &str) -> Option<String> {
    extract_hidden_input(html, "form_token")
}

/// Shared finder for hidden Drupal inputs: searches the plain
/// `name="…" value="…"` form first, then the `\u0022`-escaped variant used in
/// embedded JSON. Only the first match is taken — each form carries exactly
/// one of these inputs.
///
/// # Arguments
/// * `html` - The page HTML
/// * `name` - The input name to look for
/// # Returns
/// The input value, or `None` when absent
fn extract_hidden_input(html: &str, name: &str) -> Option<String> {
    let marker = format!("name=\"{name}\"");
    if let Some(pos) = html.find(&marker) {
        let after = &html[pos..];
        if let Some(val_pos) = after.find("value=\"") {
            let val_start = val_pos + 7;
            if let Some(val_end) = after[val_start..].find('"') {
                return Some(after[val_start..val_start + val_end].to_string());
            }
        }
    }

    let escaped_marker = format!("{name}\\u0022 value=\\u0022");
    if let Some(pos) = html.find(&escaped_marker) {
        let start = pos + escaped_marker.len();
        if let Some(end) = html[start..].find("\\u0022") {
            return Some(html[start..start + end].to_string());
        }
    }

    None
}

// ============================== BODY BUILDERS ==============================

/// Form-encodes a request body value the way the shop's own JS does: `+` for
/// spaces, `%XX` for anything outside the RFC 3986 unreserved set.
///
/// # Arguments
/// * `s` - The raw value
/// # Returns
/// The encoded value
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Builds the add-to-cart URL: `{base}{ajax_url}` plus the Drupal wrapper
/// format flag, appended to whichever query separator the endpoint carries.
///
/// # Arguments
/// * `base` - The ticketing base URL (`SHOP_BASE` in production)
/// * `ajax_url` - The `edit-add-to-cart` endpoint from the page settings
/// # Returns
/// The full POST target URL
fn build_add_to_cart_url(base: &str, ajax_url: &str) -> String {
    if ajax_url.contains('?') {
        format!("{base}{ajax_url}&_wrapper_format=drupal_ajax")
    } else {
        format!("{base}{ajax_url}?_wrapper_format=drupal_ajax")
    }
}

/// Builds the exact form-encoded body of the add-to-cart POST — the format the
/// billetterie's frontend produces when "Ajouter à mon panier" is clicked.
/// The referenced field is the seat; the slider spans the pack's price range.
///
/// # Arguments
/// * `context` - The page context extracted from the fresh resale page
/// * `seat` - The seat to add
/// # Returns
/// The x-www-form-urlencoded body
fn build_add_to_cart_body(context: &ActionContext, seat: &SeatId) -> String {
    format!(
        "nb_tickets_selector=0\
        &price_slider={min}%2C{max}\
        &sort=price-asc\
        &selected_tickets_{pack_id}={ticket_id}\
        &tickets_selected=\
        &resale_pack_selected={pack_id}\
        &form_build_id={form_build_id}\
        &form_token={form_token}\
        &form_id=hubber_resale_add_to_cart_form\
        &_triggering_element_name=add_pack\
        &_triggering_element_value=Ajouter+%C3%A0+mon+panier\
        &_drupal_ajax=1\
        &ajax_page_state%5Btheme%5D={theme}\
        &ajax_page_state%5Btheme_token%5D=\
        &ajax_page_state%5Blibraries%5D={libraries}",
        min = urlencode(&context.price_min),
        max = urlencode(&context.price_max),
        pack_id = seat.pack_id,
        ticket_id = urlencode(&seat.ticket_id),
        form_build_id = urlencode(&context.form_build_id),
        form_token = urlencode(&context.form_token),
        theme = urlencode(&context.theme),
        libraries = urlencode(&context.libraries),
    )
}

// ============================== TRANSPORT ==============================

/// Fetches a URL with the session cookie jar (reads its cookies beforehand and
/// writes them back afterwards).
///
/// # Arguments
/// * `url` - The URL to fetch
/// * `jar` - The cookie jar file, when the caller is already in a session
/// # Returns
/// The effective URL after following redirects and the fetched content
/// # Errors
/// [`CheckoutError::Network`] on any transport failure
fn fetch_with_jar(url: &str, jar: Option<&PathBuf>) -> Result<(String, String), CheckoutError> {
    let mut easy = Easy::new();
    easy.url(url)
        .map_err(|e| CheckoutError::Network(e.to_string()))?;
    if let Some(jar) = jar {
        let jar_str = jar.to_string_lossy().into_owned();
        easy.cookie_file(&jar_str)
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
        easy.cookie_jar(&jar_str)
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
    }
    easy.connect_timeout(Duration::from_secs(5))
        .map_err(|e| CheckoutError::Network(e.to_string()))?;
    easy.timeout(Duration::from_secs(30))
        .map_err(|e| CheckoutError::Network(e.to_string()))?;
    easy.follow_location(true)
        .map_err(|e| CheckoutError::Network(e.to_string()))?;

    let mut body = Vec::new();
    {
        let mut transfer = easy.transfer();
        transfer
            .write_function(|data| {
                body.extend_from_slice(data);
                Ok(data.len())
            })
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
        transfer
            .perform()
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
    }
    let final_url = match easy.effective_url() {
        Ok(Some(u)) => u.to_owned(),
        Ok(None) => url.to_owned(),
        Err(e) => return Err(CheckoutError::Network(e.to_string())),
    };
    let content = String::from_utf8(body)
        .map_err(|e| CheckoutError::Network(format!("non-utf8 response: {e}")))?;
    Ok((final_url, content))
}

/// POSTs a form body with the session cookie jar and custom headers.
///
/// # Arguments
/// * `url` - The POST target
/// * `body` - The x-www-form-urlencoded body
/// * `jar` - The cookie jar file, when the caller is already in a session
/// * `headers` - Full `Name: value` header lines (Content-Type, Referer, XHR…)
/// # Returns
/// The response content as a string
/// # Errors
/// [`CheckoutError::Network`] on transport failure or HTTP status ≥ 400
fn post_form(
    url: &str,
    body: &str,
    jar: Option<&PathBuf>,
    headers: &[String],
) -> Result<String, CheckoutError> {
    let mut easy = Easy::new();
    easy.url(url)
        .map_err(|e| CheckoutError::Network(e.to_string()))?;
    easy.post(true)
        .map_err(|e| CheckoutError::Network(e.to_string()))?;
    easy.post_fields_copy(body.as_bytes())
        .map_err(|e| CheckoutError::Network(e.to_string()))?;
    if let Some(jar) = jar {
        let jar_str = jar.to_string_lossy().into_owned();
        easy.cookie_file(&jar_str)
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
        easy.cookie_jar(&jar_str)
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
    }
    easy.follow_location(true)
        .map_err(|e| CheckoutError::Network(e.to_string()))?;

    let mut list = List::new();
    for header in headers {
        list.append(header)
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
    }
    easy.http_headers(list)
        .map_err(|e| CheckoutError::Network(e.to_string()))?;

    let mut response = Vec::new();
    {
        let mut transfer = easy.transfer();
        transfer
            .write_function(|data| {
                response.extend_from_slice(data);
                Ok(data.len())
            })
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
        transfer
            .perform()
            .map_err(|e| CheckoutError::Network(e.to_string()))?;
    }
    let status = easy
        .response_code()
        .map_err(|e| CheckoutError::Network(e.to_string()))?;
    if status >= 400 {
        return Err(CheckoutError::Network(format!("HTTP {status}")));
    }
    String::from_utf8(response)
        .map_err(|e| CheckoutError::Network(format!("non-utf8 response: {e}")))
}

/// Builds the header set of the add-to-cart AJAX POST — the exact headers the
/// site's own jQuery frontend sends (the shop may validate them).
///
/// # Arguments
/// * `referer` - The `Referer` value (the resale page URL without its query)
/// # Returns
/// The header lines ready for [`post_form`]
fn add_to_cart_headers(referer: &str) -> Vec<String> {
    vec![
        "Content-Type: application/x-www-form-urlencoded; charset=UTF-8".to_string(),
        "Accept: application/json, text/javascript, */*; q=0.01".to_string(),
        "X-Requested-With: XMLHttpRequest".to_string(),
        format!("Referer: {referer}"),
    ]
}

// ============================== SESSION ==============================

/// One short-lived authenticated shop session: the login flow opens it, seats
/// are added within it, dropping it deletes the cookie jar.
#[derive(Debug)]
pub struct ShopSession {
    /// The ticketing base URL the session belongs to.
    base: String,
    /// The session cookie jar file (temp file, removed on drop).
    jar: PathBuf,
}

impl ShopSession {
    /// Runs the two-step Drupal login and verifies the authenticated session
    /// on the basket page.
    ///
    /// # Arguments
    /// * `base` - The ticketing base URL (`SHOP_BASE` in production; tests
    ///   pass a mocked server URL)
    /// * `email` - The shop account email
    /// * `password` - The shop account password
    /// # Returns
    /// The authenticated session (cookie jar carried inside)
    /// # Errors
    /// [`CheckoutError::WaitingQueue`] when the shop is under load,
    /// [`CheckoutError::Login`] when the flow fails (unknown account, missing
    /// form id, basket never looks authenticated),
    /// [`CheckoutError::Network`] on transport failure
    pub fn login(base: &str, email: &str, password: &str) -> Result<Self, CheckoutError> {
        log::info!("[CHECKOUT] shop login started");
        let jar = fresh_jar_path();
        let headers = |referer: String| -> Vec<String> {
            vec![
                "Content-Type: application/x-www-form-urlencoded".to_string(),
                referer,
            ]
        };

        // Step 1: fetch the login page and extract the initial form id.
        let (final_url, page) = fetch_with_jar(&format!("{base}{LOGIN_PATH}"), Some(&jar))?;
        if is_waiting_queue(&final_url, &page) {
            return Err(CheckoutError::WaitingQueue);
        }
        let build_id = extract_form_build_id(&page).ok_or_else(|| {
            CheckoutError::Login("form_build_id not found on the login page".to_string())
        })?;

        // Step 2: email step — a response containing /register means unknown
        // account.
        let email_body = format!(
            "name={}&form_build_id={}&form_id=user_login_form&op=Continuer",
            urlencode(email),
            urlencode(&build_id),
        );
        let step2 = post_form(
            &format!("{base}{LOGIN_PATH}"),
            &email_body,
            Some(&jar),
            &headers(format!("Referer: {base}{LOGIN_PATH}")),
        )?;
        if step2.contains("/register") {
            return Err(CheckoutError::Login(
                "unknown account for this email".to_string(),
            ));
        }
        let build_id2 = extract_form_build_id(&step2).ok_or_else(|| {
            CheckoutError::Login("form_build_id not found after the email step".to_string())
        })?;

        // Step 3: password step — an HTTP failure here is fine, only the
        // basket check tells the truth.
        let password_body = format!(
            "name={}&pass={}&form_build_id={}&form_id=user_login_form&op=Se+connecter",
            urlencode(email),
            urlencode(password),
            urlencode(&build_id2),
        );
        let _ = post_form(
            &format!("{base}{LOGIN_PATH}"),
            &password_body,
            Some(&jar),
            &headers(format!("Referer: {base}{LOGIN_PATH}")),
        )?;

        // Step 4: verify the session on the basket page.
        let (final_url, basket) = fetch_with_jar(&format!("{base}{BASKET_PATH}"), Some(&jar))?;
        if is_waiting_queue(&final_url, &basket) {
            return Err(CheckoutError::WaitingQueue);
        }
        if !(basket.contains("user-logged-in") || basket.contains("Se déconnecter")) {
            return Err(CheckoutError::Login(
                "basket page does not look authenticated".to_string(),
            ));
        }

        log::info!("[CHECKOUT] shop login succeeded");
        Ok(Self {
            base: base.to_string(),
            jar,
        })
    }

    /// Adds one seat to the session's cart: refreshes the resale page (fresh
    /// context + seat presence guard), then POSTs the add-to-cart request.
    ///
    /// Callers add several seats (a group request) by calling this repeatedly
    /// within the same session; each call self-resynchronizes the Drupal form
    /// ids against the fresh page.
    ///
    /// # Arguments
    /// * `resale_url` - The absolute resale product URL
    /// * `seat` - The seat to add (pack + ticket from the fresh scan)
    /// # Errors
    /// [`CheckoutError::WaitingQueue`] when the shop is under load,
    /// [`CheckoutError::MissingContext`] when the fresh page lost its
    /// add-to-cart machinery (resale closed),
    /// [`CheckoutError::SeatGone`] when the seat vanished from the page,
    /// [`CheckoutError::ExpiredForm`] when the POST is rejected as expired,
    /// [`CheckoutError::Network`] on transport failure
    pub fn add_seat(&self, resale_url: &str, seat: &SeatId) -> Result<(), CheckoutError> {
        // Step 1: refresh the page within the session (tokens are per render).
        let (final_url, html) = fetch_with_jar(resale_url, Some(&self.jar))?;
        if is_waiting_queue(&final_url, &html) {
            return Err(CheckoutError::WaitingQueue);
        }

        // Step 2: rebuild the context and guard the seat presence.
        let context = extract_action_context(&html).ok_or(CheckoutError::MissingContext)?;
        let present =
            parse_seat::parse_pack_ticket_ids(&html)
                .iter()
                .any(|(pack_id, ticket_ids)| {
                    *pack_id == seat.pack_id && ticket_ids.iter().any(|t| t == &seat.ticket_id)
                });
        if !present {
            return Err(CheckoutError::SeatGone);
        }

        // Step 3: POST the add-to-cart request (XHR header set of the site).
        let url = build_add_to_cart_url(&self.base, &context.ajax_url);
        let body = build_add_to_cart_body(&context, seat);
        let referer = format!(
            "{}{}",
            self.base,
            context.ajax_url.split('?').next().unwrap_or("")
        );
        let response = post_form(&url, &body, Some(&self.jar), &add_to_cart_headers(&referer))?;
        log::info!("[CHECKOUT] add-to-cart response: {} bytes", response.len());

        // Step 4: detect the stale/invalid-session rejection.
        if response.contains("p\u{e9}rim\u{e9}")
            || response.contains("p%C3%A9rim%C3%A9")
            || (response.contains("alert-danger") && response.contains("formulaire"))
        {
            return Err(CheckoutError::ExpiredForm);
        }
        Ok(())
    }
}

impl Drop for ShopSession {
    /// Deletes the session cookie jar (nothing is left behind, even after a
    /// failed login thanks to early drops).
    fn drop(&mut self) {
        if self.jar.try_exists().is_ok_and(|exists| exists) {
            let _ = std::fs::remove_file(&self.jar);
        }
    }
}

/// Builds a unique temp path for one session's cookie jar.
///
/// # Returns
/// A temp path not used by any other session (pid + nanos)
fn fresh_jar_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "ovalbot_sr_session_{}_{}.jar",
        std::process::id(),
        nanos
    ))
}

// ============================== TESTS ==============================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::gz_html;
    use std::io::{BufRead, Read, Write};
    use std::net::TcpListener;
    use std::sync::LazyLock;
    use std::sync::{Arc, Mutex};
    use std::thread;

    // Captured live from the open UBB resale page on 2026-10-21 (anonymous
    // capture: no form_token, user.uid = 0).
    static FIXTURE_RESALE: LazyLock<String> = LazyLock::new(|| {
        gz_html(include_bytes!(
            "../../app/clubs/larochelle/fixtures/seat_page_resale_open.html.gz"
        ))
    });

    // ============================ EXTRACTION ============================

    // The committed fixture carries the real form_build_id of the capture.
    #[test]
    fn extract_form_build_id_from_the_fixture() {
        assert_eq!(
            extract_form_build_id(&FIXTURE_RESALE).as_deref(),
            Some("form-H49AQlTxmNGR8pR0pTCLTORjaL6xtVQHChk2QNkR1c4")
        );
    }

    // Anonymous captures ship no form_token: the extractor reports None and
    // the POST is built with an empty value.
    #[test]
    fn extract_form_token_is_absent_in_anonymous_captures() {
        assert_eq!(extract_form_token(&FIXTURE_RESALE), None);
    }

    // Both the plain and JSON-escaped form variants are read.
    #[test]
    fn extract_hidden_input_reads_both_variants() {
        let plain = r#"<input name="form_token" value="tok-123">"#;
        assert_eq!(extract_form_token(plain).as_deref(), Some("tok-123"));

        let escaped = r#"{"form_build_id\u0022 value=\u0022form-456\u0022":"x"}"#;
        assert_eq!(extract_form_build_id(escaped).as_deref(), Some("form-456"));

        assert_eq!(extract_form_build_id("<html>no form</html>"), None);
    }

    // The fixture context carries the live values of the capture.
    #[test]
    fn action_context_from_the_fixture() {
        let context = extract_action_context(&FIXTURE_RESALE).expect("context");
        assert!(
            context
                .ajax_url
                .starts_with("/fr/product/1357/revente_j7_24_25_oct_stade_rochelais_ubb")
        );
        assert!(context.ajax_url.contains("ajax_form=1"));
        assert!(!context.libraries.is_empty());
        assert_eq!(context.theme, "hubber_reference8");
        assert_eq!(context.price_min, "44.00");
        assert_eq!(context.price_max, "59");
        assert_eq!(
            context.form_build_id,
            "form-H49AQlTxmNGR8pR0pTCLTORjaL6xtVQHChk2QNkR1c4"
        );
        assert_eq!(context.form_token, "");
    }

    // A closed resale page carries no slider / settings: no add-to-cart.
    #[test]
    fn action_context_requires_the_page_plumbing() {
        assert!(extract_action_context("<html><body></body></html>").is_none());
        assert!(extract_action_context("Move to waiting queue").is_none());
    }

    // urlencode escapes like a form: spaces to `+`, accents percent-encoded.
    #[test]
    fn urlencode_escapes_form_values() {
        assert_eq!(urlencode("user@example.com"), "user%40example.com");
        assert_eq!(urlencode("mon mot de passe"), "mon+mot+de+passe");
        assert_eq!(urlencode("form-abc_1.~"), "form-abc_1.~");
        assert_eq!(urlencode("44.00"), "44.00");
    }

    // The POST body carries the seat, the fresh ids and the replayed state.
    #[test]
    fn add_to_cart_body_contains_every_required_field() {
        let context = extract_action_context(&FIXTURE_RESALE).expect("context");
        let seat = SeatId {
            pack_id: 33850,
            ticket_id: "49432".to_string(),
        };
        let body = build_add_to_cart_body(&context, &seat);
        assert!(body.contains("selected_tickets_33850=49432"));
        assert!(body.contains("resale_pack_selected=33850"));
        assert!(body.contains("price_slider=44.00%2C59"));
        assert!(body.contains(&format!(
            "form_build_id={}",
            urlencode(&context.form_build_id)
        )));
        assert!(body.contains("form_token="));
        assert!(body.contains("form_id=hubber_resale_add_to_cart_form"));
        assert!(body.contains("_triggering_element_name=add_pack"));
        assert!(body.contains("_triggering_element_value=Ajouter+%C3%A0+mon+panier"));
        assert!(body.contains("_drupal_ajax=1"));
        assert!(body.contains("ajax_page_state%5Btheme%5D=hubber_reference8"));
        assert!(body.contains("ajax_page_state%5Btheme_token%5D="));
        assert!(body.contains(&format!(
            "ajax_page_state%5Blibraries%5D={}",
            urlencode(&context.libraries)
        )));
    }

    // Query-separator normalization: the endpoint already carries `?ajax_form=1`.
    #[test]
    fn add_to_cart_url_appends_the_wrapper_format() {
        assert_eq!(
            build_add_to_cart_url("https://shop.example", "/fr/product/1/r?ajax_form=1"),
            "https://shop.example/fr/product/1/r?ajax_form=1&_wrapper_format=drupal_ajax"
        );
        assert_eq!(
            build_add_to_cart_url("https://shop.example", "/fr/product/1/r"),
            "https://shop.example/fr/product/1/r?_wrapper_format=drupal_ajax"
        );
    }

    // The XHR header set of the site's own frontend is replayed.
    #[test]
    fn add_to_cart_headers_carry_the_xhr_markers() {
        let headers = add_to_cart_headers("https://shop.example/fr/product/1");
        assert!(headers.iter().any(|h| h.contains("charset=UTF-8")));
        assert!(
            headers
                .iter()
                .any(|h| h.starts_with("Accept: application/json"))
        );
        assert!(
            headers
                .iter()
                .any(|h| h.contains("X-Requested-With: XMLHttpRequest"))
        );
        assert!(
            headers
                .iter()
                .any(|h| h.contains("Referer: https://shop.example"))
        );
    }

    // ============================ MOCKED TRANSPORT ============================

    /// Shorthand for a mocked response: body + status in one expression.
    fn page(body: impl Into<String>, status: u16) -> (String, u16) {
        (body.into(), status)
    }

    /// Mocked HTTP server answering the given canned bodies in order (one per
    /// connection), recording every request (its first line and its body).
    ///
    /// Handles the `Expect: 100-continue` preamble curl uses for bodies over
    /// 1 KB (the replayed `libraries` string makes the add-to-cart POST one).
    fn serve_mock(responses: Vec<(String, u16)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = Arc::clone(&requests);
        thread::spawn(move || {
            for (index, stream) in listener.incoming().enumerate() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                // Read the request fully (headers + Content-Length body).
                let mut reader = std::io::BufReader::new(&mut stream);
                let mut headers = String::new();
                let mut content_length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_length = rest.trim().parse().unwrap_or(0);
                    }
                    headers.push_str(&line);
                }
                let mut body = vec![0u8; content_length];
                reader.read_exact(&mut body).ok();
                drop(reader);

                let logged = format!("{}\nBODY: {}", headers, String::from_utf8_lossy(&body));
                log.lock().expect("log").push(logged);

                // Proceed the 100-continue preamble so curl sends its body.
                if headers
                    .to_ascii_lowercase()
                    .contains("expect: 100-continue")
                {
                    let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
                }

                let (body, status) = responses
                    .get(index)
                    .cloned()
                    .unwrap_or(page("mock exhausted", 500));
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{addr}"), requests)
    }

    // A login page and a step-2 page each carrying a fresh build id.
    const LOGIN_PAGE: &str =
        "<html><form><input name=\"form_build_id\" value=\"form-login-1\"></form></html>";
    const STEP2_PAGE: &str =
        "<html><form><input name=\"form_build_id\" value=\"form-step2-2\"></form></html>";
    const BASKET_AUTHENTICATED: &str =
        "<html><body class=\"user-logged-in\">Se déconnecter</body></html>";
    const BASKET_ANONYMOUS: &str = "<html><body>Connexion</body></html>";
    const ADD_OK: &str = "[]";
    const ADD_EXPIRED: &str =
        r#"{"markup":"<div class=\"alert-danger\">Un message formulaire</div>"}"#;
    const QUEUED: &str = "Move to waiting queue";

    // Full login flow: 4 requests in the right order with the right bodies.
    #[test]
    fn login_succeeds_over_the_mocked_shop() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
        ]);
        let session = ShopSession::login(&base, "user@example.com", "secret").expect("login");
        let log = requests.lock().expect("log").join("\n---\n");

        assert!(log.contains("GET /fr/user/login"), "first request: {log}");
        assert!(
            log.contains("name=user%40example.com")
                && log.contains("form_build_id=form-login-1")
                && log.contains("op=Continuer"),
            "email step: {log}"
        );
        assert!(
            log.contains("pass=secret")
                && log.contains("form_build_id=form-step2-2")
                && log.contains("op=Se+connecter"),
            "password step: {log}"
        );
        assert!(log.contains("GET /fr/basket"), "basket check: {log}");
        assert!(session.jar.exists(), "cookie jar kept alive while in use");
    }

    // A basket page without the authenticated markers fails the login.
    #[test]
    fn login_fails_when_the_basket_stays_anonymous() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_ANONYMOUS, 200),
        ]);
        let error = ShopSession::login(&base, "a@b.c", "x").expect_err("must fail");
        assert_eq!(
            error,
            CheckoutError::Login("basket page does not look authenticated".to_string())
        );
        assert_eq!(requests.lock().expect("log").len(), 4);
    }

    // The email step answering /register means unknown account.
    #[test]
    fn login_fails_when_the_account_is_unknown() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page("<html><a href=\"/register\">Inscrivez-vous</a></html>", 200),
        ]);
        let error = ShopSession::login(&base, "ghost@b.c", "x").expect_err("must fail");
        assert_eq!(
            error,
            CheckoutError::Login("unknown account for this email".to_string())
        );
        assert_eq!(requests.lock().expect("log").len(), 2);
    }

    // The login flow reports the waiting queue distinctly.
    #[test]
    fn login_reports_the_waiting_queue() {
        let (base, requests) = serve_mock(vec![page(QUEUED, 307)]);
        assert_eq!(
            ShopSession::login(&base, "a@b.c", "x").expect_err("must fail"),
            CheckoutError::WaitingQueue
        );
        assert_eq!(requests.lock().expect("log").len(), 1);
    }

    // A transport failure (connection refused) maps to Network.
    #[test]
    fn login_reports_network_failures() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        drop(listener);
        assert!(matches!(
            ShopSession::login(&format!("http://127.0.0.1:{port}"), "a@b.c", "x"),
            Err(CheckoutError::Network(_))
        ));
    }

    // Adding a seat inside one session: fresh page → context + presence guard
    // → POST with the seat in the body and the XHR headers.
    #[test]
    fn add_seat_posts_the_add_to_cart_request() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
            page(ADD_OK, 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );

        let session = ShopSession::login(&base, "user@example.com", "secret").expect("login");
        let seat = SeatId {
            pack_id: 33850,
            ticket_id: "49432".to_string(),
        };
        session.add_seat(&resale_url, &seat).expect("add-to-cart");

        // The POST landed on the fixture page path and carries the seat.
        let log = requests.lock().expect("log");
        let post = log
            .iter()
            .find(|r| r.contains("POST /fr/product/1357"))
            .expect("add-to-cart POST recorded");
        assert!(post.contains("BODY: ") && post.contains("selected_tickets_33850=49432"));
        assert!(post.contains("resale_pack_selected=33850"));
        assert!(post.contains("price_slider=44.00%2C59"));
        assert!(post.contains("X-Requested-With: XMLHttpRequest"));
        assert!(
            post.contains("Referer: ") && !post.contains("Referer: \r\n"),
            "refer header present"
        );
        assert_eq!(log.len(), 6);
    }

    // A stale-session rejection body is mapped to ExpiredForm.
    #[test]
    fn add_seat_reports_the_expired_form_rejection() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
            page(ADD_EXPIRED, 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        let seat = SeatId {
            pack_id: 33850,
            ticket_id: "49432".to_string(),
        };
        assert_eq!(
            session.add_seat(&resale_url, &seat).expect_err("must fail"),
            CheckoutError::ExpiredForm
        );
        assert_eq!(requests.lock().expect("log").len(), 6);
    }

    // A seat that vanished from the fresh page is reported as SeatGone (no
    // add-to-cart POST is sent).
    #[test]
    fn add_seat_reports_a_seat_gone() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        let seat = SeatId {
            pack_id: 999,
            ticket_id: "0".to_string(),
        };
        assert_eq!(
            session.add_seat(&resale_url, &seat).expect_err("must fail"),
            CheckoutError::SeatGone
        );
        // 4 login requests + 1 page fetch; no POST.
        assert_eq!(requests.lock().expect("log").len(), 5);
    }

    // The waiting queue during the seat addition is reported distinctly.
    #[test]
    fn add_seat_reports_the_waiting_queue() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(QUEUED, 307),
        ]);
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        let seat = SeatId {
            pack_id: 33850,
            ticket_id: "49432".to_string(),
        };
        assert_eq!(
            session
                .add_seat(&format!("{base}/fr/product/1/resale"), &seat)
                .expect_err("must fail"),
            CheckoutError::WaitingQueue
        );
        assert_eq!(requests.lock().expect("log").len(), 5);
    }

    // Dropping the session deletes the cookie jar file.
    #[test]
    fn session_drop_deletes_the_jar() {
        let jar = {
            let (base, _requests) = serve_mock(vec![
                page(LOGIN_PAGE, 200),
                page(STEP2_PAGE, 200),
                page("<html>connecté</html>", 200),
                page(BASKET_AUTHENTICATED, 200),
            ]);
            let session = ShopSession::login(&base, "user@example.com", "secret").expect("login");
            let jar = session.jar.clone();
            assert!(jar.exists());
            drop(session);
            jar
        };
        assert!(!jar.exists(), "cookie jar deleted on drop");
    }
}
