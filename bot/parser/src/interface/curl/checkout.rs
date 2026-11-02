//! Shop checkout for Stade Rochelais: authenticated login and the Drupal AJAX
//! add-to-cart flow.
//!
//! This module is the only consumer-facing piece of the purchase feature: the
//! OvalBot orchestrator opens a [`ShopSession`] with credentials, then calls
//! [`ShopSession::add_pack`] once per resale pack (a pack is atomic: a 2-ticket
//! listing cannot be split) inside the same login session. The passive scan
//! never touches it.
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
//! - On an available resale the listing and the price slider are injected by a
//!   **BigPipe lazy builder** after the initial render, so the fresh page may
//!   legitimately carry no `data-json`: the pack presence guard only runs when
//!   a listing is actually present, and the escaped slider markup is read from
//!   the raw HTML (fallback: the pack's own price span).
//! - The add verdict comes from the AJAX command list: the pack row gains
//!   `added-to-cart` (or its JSON says `is_already_in_cart`, or the response
//!   ends with `refreshCartTimeout`). Anything else maps to
//!   [`CheckoutError::SeatGone`] — the pack never reached the cart.
//! - A stale/invalid-session rejection is detected by its body markers
//!   ("périmé" / `alert-danger` + `formulaire`) → [`CheckoutError::ExpiredForm`].
//! - Sessions are deliberately short-lived: the cookie jar lives in a temp
//!   file and is deleted when the [`ShopSession`] is dropped.
//! - Set `CHECKOUT_DUMP_DIR` to dump the page and/or AJAX response on a
//!   checkout failure (diagnostic only, nothing is written otherwise).

use crate::app::clubs::larochelle::parse_seat;
use crate::interface::match_manager::is_waiting_queue;
use curl::easy::{Easy, List};
use scraper::{Html, Selector};
use std::path::{Path, PathBuf};
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

/// Class marker of the sold-out resale page variant: the listing and the
/// price slider are gone, only "Aucun billet n'est actuellement disponible"
/// remains. Treated as seat churn (`SeatGone`), never as a missing form.
const OUT_OF_STOCK_MARKER: &str = "product-not-on-sale-info";

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
    /// The resale pack is no longer listed on the fresh page — someone took it
    /// (a pack is sold atomically, all its tickets disappear together).
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
            CheckoutError::SeatGone => f.write_str("pack no longer present on the resale page"),
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

/// Parses the page's `drupal-settings-json` script into a JSON value.
///
/// The structure evolves between site releases, so the settings are navigated
/// lazily (a strict typed struct breaks on any new key shape).
///
/// # Arguments
/// * `html` - The resale page HTML
/// # Returns
/// The parsed settings, or `None` when the script is absent or not JSON
fn parse_drupal_settings(html: &str) -> Option<serde_json::Value> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("script[data-drupal-selector='drupal-settings-json']").unwrap();
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>();
    serde_json::from_str(&text).ok()
}

/// Extracts the [`ActionContext`] of a resale page: the `drupal-settings-json`
/// values plus the form ids. `None` when the page misses a required piece
/// (settings or `form_build_id`) — e.g. a page whose lazy-built form never
/// arrived.
///
/// On an available resale the form, its price slider and the `edit-add-to-cart`
/// AJAX entry are injected by a BigPipe lazy builder, so the page may legally
/// carry none of them: the slider falls back to the raw HTML (escaped chunks
/// included), the AJAX endpoint falls back to the product URL with
/// `ajax_form=1`, and the slider bounds are left empty for the caller.
///
/// # Arguments
/// * `html` - The resale page HTML
/// * `fallback_url` - The resale product URL, used to rebuild the AJAX endpoint
///   when the settings JSON does not carry `edit-add-to-cart`
/// # Returns
/// The page context, or `None` when the add-to-cart machinery is absent
pub fn extract_action_context(html: &str, fallback_url: Option<&str>) -> Option<ActionContext> {
    let settings = parse_drupal_settings(html)?;

    let ajax_url = settings
        .pointer("/ajax/edit-add-to-cart/url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .or_else(|| fallback_url.map(default_ajax_url))?;

    let libraries = settings
        .pointer("/ajaxPageState/libraries")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let theme = settings
        .pointer("/ajaxPageState/theme")
        .and_then(serde_json::Value::as_str)
        .filter(|theme| !theme.is_empty())
        .unwrap_or(DEFAULT_THEME)
        .to_string();

    let slider_selector = Selector::parse("input.hubber-slider-input").unwrap();
    let (price_min, price_max) = Html::parse_document(html)
        .select(&slider_selector)
        .next()
        .and_then(|slider| {
            Some((
                slider.value().attr("data-min")?.to_string(),
                slider.value().attr("data-max")?.to_string(),
            ))
        })
        .or_else(|| extract_slider_bounds(html))
        .unwrap_or_default();

    // form_build_id is required (the form must be on the page); form_token is
    // optional — anonymous pages ship without one and the POST still works.
    let form_build_id = extract_form_build_id(html)?;
    let form_token = extract_form_token(html).unwrap_or_default();

    Some(ActionContext {
        ajax_url,
        libraries,
        theme,
        price_min,
        price_max,
        form_build_id,
        form_token,
    })
}

/// The add-to-cart AJAX endpoint of a product page, used when the settings JSON
/// does not carry `edit-add-to-cart` (BigPipe pages): the product URL with
/// `ajax_form=1`, exactly what the captures show.
///
/// # Arguments
/// * `resale_url` - The resale product URL
/// # Returns
/// The product URL with the `ajax_form=1` query parameter
fn default_ajax_url(resale_url: &str) -> String {
    if resale_url.contains('?') {
        format!("{resale_url}&ajax_form=1")
    } else {
        format!("{resale_url}?ajax_form=1")
    }
}

/// One-line report of the context pieces a page carries, logged when the
/// add-to-cart context cannot be built (no dump needed to know what is missing).
///
/// # Arguments
/// * `html` - The resale page HTML
/// # Returns
/// A compact `key=value` diagnostic
fn context_diagnostic(html: &str) -> String {
    let settings = parse_drupal_settings(html);
    let has_ajax_entry = settings
        .as_ref()
        .and_then(|value| value.pointer("/ajax/edit-add-to-cart/url"))
        .and_then(serde_json::Value::as_str)
        .is_some();
    let has_libraries = settings
        .as_ref()
        .and_then(|value| value.pointer("/ajaxPageState/libraries"))
        .and_then(serde_json::Value::as_str)
        .is_some();
    format!(
        "bytes={} settings={} ajax_entry={has_ajax_entry} libraries={has_libraries} \
         form_build_id={} slider={}",
        html.len(),
        settings.is_some(),
        extract_form_build_id(html).is_some(),
        extract_slider_bounds(html).is_some(),
    )
}

/// Reads the price-slider bounds from the raw HTML, including the
/// BigPipe/AJAX-escaped markup the site injects after the initial render
/// (`data-min=\u002249.00\u0022`).
///
/// # Arguments
/// * `html` - The resale page HTML (raw, with its escaped chunks)
/// # Returns
/// `(min, max)` as printed on the page, or `None` when no slider is present
fn extract_slider_bounds(html: &str) -> Option<(String, String)> {
    let start = html.find("hubber-slider-input")?;
    let window = &html[start..(start + 800).min(html.len())];
    Some((
        attribute_value(window, "data-min")?,
        attribute_value(window, "data-max")?,
    ))
}

/// Reads one `name="value"` (or `name=\u0022value\u0022`) attribute from a
/// substring.
///
/// # Arguments
/// * `window` - The substring to scan
/// * `name` - The attribute name, without the trailing `=`
/// # Returns
/// The attribute value, or `None` when absent or unquoted
fn attribute_value(window: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=");
    let pos = window.find(&marker)? + marker.len();
    let rest = &window[pos..];
    let quote = if rest.starts_with("\\u0022") {
        "\\u0022"
    } else if rest.starts_with('"') {
        "\""
    } else {
        return None;
    };
    let value = &rest[quote.len()..];
    let end = value.find(quote)?;
    Some(value[..end].to_string())
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
///
/// The billetterie sells a resale **pack atomically**: every ticket of the pack
/// is submitted at once, comma-joined in `selected_tickets_{pack_id}`
/// (`t1,t2`), and `resale_pack_selected` names the pack. A single-ticket pack
/// sends one id. Payload captured live on 2026-11-01: a 2-ticket pack went out
/// as `selected_tickets_34681=50587%2C50590`.
///
/// # Arguments
/// * `context` - The page context extracted from the fresh resale page
/// * `pack_id` - The resale pack to add
/// * `tickets` - Every ticket id of that pack
/// # Returns
/// The x-www-form-urlencoded body
fn build_add_to_cart_body(context: &ActionContext, pack_id: u64, tickets: &[String]) -> String {
    let selected = tickets
        .iter()
        .map(|ticket| urlencode(ticket))
        .collect::<Vec<_>>()
        .join("%2C");
    format!(
        "nb_tickets_selector=0\
        &price_slider={min}%2C{max}\
        &sort=price-asc\
        &selected_tickets_{pack_id}={selected}\
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
    jar: SessionJar,
}

/// One session's cookie jar temp file, removed on drop.
///
/// Created before the first request of a login so that **every** path —
/// including a refused, queued or failed login — deletes the file; nothing is
/// left behind in the temp directory.
#[derive(Debug)]
struct SessionJar(PathBuf);

impl SessionJar {
    /// Creates a fresh (not yet written) jar path for this process.
    fn fresh() -> Self {
        Self(fresh_jar_path())
    }

    /// The jar path handed to curl.
    fn path(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for SessionJar {
    /// Deletes the jar file (whatever curl wrote into it).
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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
        let jar = SessionJar::fresh();
        let headers = |referer: String| -> Vec<String> {
            vec![
                "Content-Type: application/x-www-form-urlencoded".to_string(),
                referer,
            ]
        };

        // Step 1: fetch the login page and extract the initial form id.
        let (final_url, page) = fetch_with_jar(&format!("{base}{LOGIN_PATH}"), Some(jar.path()))?;
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
            Some(jar.path()),
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
            Some(jar.path()),
            &headers(format!("Referer: {base}{LOGIN_PATH}")),
        )?;

        // Step 4: verify the session on the basket page.
        let (final_url, basket) =
            fetch_with_jar(&format!("{base}{BASKET_PATH}"), Some(jar.path()))?;
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

    /// Adds one resale **pack** to the session's cart: refreshes the resale page
    /// (pack presence guard first, then a fresh add-to-cart context), then POSTs
    /// the add-to-cart request carrying every ticket of the pack.
    ///
    /// The billetterie sells packs atomically (a 2-ticket resale cannot be
    /// split), so the pack is the add unit. Callers add several packs (a group
    /// request) by calling this repeatedly within the same session; each call
    /// self-resynchronizes the Drupal form ids against the fresh page.
    ///
    /// # Arguments
    /// * `resale_url` - The absolute resale product URL
    /// * `pack_id` - The resale pack to add (from the fresh scan)
    /// * `tickets` - Every ticket id of that pack (the whole pack)
    /// * `price_range` - `price_slider` fallback, used when the page does not
    ///   expose its lazily-built price slider
    /// # Errors
    /// [`CheckoutError::WaitingQueue`] when the shop is under load,
    /// [`CheckoutError::SeatGone`] when the pack vanished, changed composition,
    /// the resale sold out, or the POST was rejected (no cart confirmation),
    /// [`CheckoutError::MissingContext`] when the page lost its add-to-cart
    /// machinery (site layout change),
    /// [`CheckoutError::ExpiredForm`] when the POST is rejected as expired,
    /// [`CheckoutError::Network`] on transport failure
    ///
    /// With `CHECKOUT_DUMP_DIR` set, every attempt writes the fetched page, the
    /// AJAX response and an `attempt` metadata file there (diagnostic only).
    pub fn add_pack(
        &self,
        resale_url: &str,
        pack_id: u64,
        tickets: &[String],
        price_range: (String, String),
    ) -> Result<(), CheckoutError> {
        let dump = AttemptDump::from_env(pack_id, tickets, &price_range);

        // Step 1: refresh the page within the session (tokens are per render).
        let (final_url, html) = fetch_with_jar(resale_url, Some(self.jar.path()))?;
        if is_waiting_queue(&final_url, &html) {
            dump.finish("waiting_queue", Some(&html), None);
            return Err(CheckoutError::WaitingQueue);
        }

        // Step 2: guard the pack presence, but only when the page really lists
        // packs. On an available resale the listing (and the price slider) are
        // injected by a BigPipe lazy builder after the initial render, so a
        // curl fetch legitimately sees none: in that case the plan's ids (fresh
        // from the scan) are trusted and the POST verdict decides.
        let listed = parse_seat::parse_pack_ticket_ids(&html);
        if !listed.is_empty() {
            let fresh = listed
                .iter()
                .find(|(listed_pack, _)| *listed_pack == pack_id);
            let Some((_, listed_tickets)) = fresh else {
                log::debug!("[CHECKOUT] pack {pack_id} absent from the listed packs");
                dump.finish("pack_absent", Some(&html), None);
                return Err(CheckoutError::SeatGone);
            };
            if html.contains(OUT_OF_STOCK_MARKER) || !same_tickets(listed_tickets, tickets) {
                // The pack disappeared or was re-minted with another
                // composition: the plan is stale, the next wave re-plans from
                // fresh seats.
                log::debug!(
                    "[CHECKOUT] pack {pack_id} changed or gone (wanted {tickets:?}, found {listed_tickets:?})"
                );
                dump.finish("pack_changed", Some(&html), None);
                return Err(CheckoutError::SeatGone);
            }
        }

        // Step 3: rebuild the add-to-cart context (settings + form id).
        let Some(mut context) = extract_action_context(&html, Some(resale_url)) else {
            log::warn!(
                "[CHECKOUT] add-to-cart context missing from the resale page ({})",
                context_diagnostic(&html)
            );
            dump.finish("missing_context", Some(&html), None);
            return Err(CheckoutError::MissingContext);
        };
        if context.price_min.is_empty() || context.price_max.is_empty() {
            context.price_min = price_range.0;
            context.price_max = price_range.1;
        }

        // Step 4: POST the add-to-cart request (XHR header set of the site).
        let url = build_add_to_cart_url(&self.base, &context.ajax_url);
        let body = build_add_to_cart_body(&context, pack_id, tickets);
        let referer = format!(
            "{}{}",
            self.base,
            context.ajax_url.split('?').next().unwrap_or("")
        );
        let response = match post_form(
            &url,
            &body,
            Some(self.jar.path()),
            &add_to_cart_headers(&referer),
        ) {
            Ok(response) => response,
            Err(e) => {
                dump.finish("post_network", Some(&html), None);
                return Err(e);
            }
        };
        log::info!("[CHECKOUT] add-to-cart response: {} bytes", response.len());

        // Step 5: detect the stale/invalid-session rejection.
        if response.contains("p\u{e9}rim\u{e9}")
            || response.contains("p%C3%A9rim%C3%A9")
            || (response.contains("alert-danger") && response.contains("formulaire"))
        {
            dump.finish("expired_form", Some(&html), Some(&response));
            return Err(CheckoutError::ExpiredForm);
        }

        // Step 6: the AJAX command list is the only trustworthy verdict — an
        // accepted add re-renders the pack `added-to-cart`; anything else means
        // the pack was gone between the plan and the POST.
        if !pack_added(&response, pack_id) {
            log::debug!("[CHECKOUT] pack {pack_id} add not confirmed by the response");
            dump.finish("rejected", Some(&html), Some(&response));
            return Err(CheckoutError::SeatGone);
        }
        dump.finish("added", Some(&html), Some(&response));
        Ok(())
    }
}

/// Whether the add-to-cart AJAX response confirms the pack landed in the cart.
///
/// Success markers captured live on 2026-11-01 (pack 34720): the pack row gets
/// the `added-to-cart` class, its re-rendered JSON carries
/// `is_already_in_cart`, and the response ends with `refreshCartTimeout`. A
/// response with none of them is a rejection, mapped to `SeatGone` (the next
/// wave re-plans).
///
/// # Arguments
/// * `response` - The AJAX response body
/// * `pack_id` - The pack that was submitted
/// # Returns
/// `true` when the response carries a success marker for that pack
fn pack_added(response: &str, pack_id: u64) -> bool {
    response.contains(&pack_id.to_string())
        && (response.contains("added-to-cart")
            || response.contains("is_already_in_cart")
            || response.contains("refreshCartTimeout"))
}

/// One diagnostic capture of an add attempt, enabled by `CHECKOUT_DUMP_DIR`.
///
/// Every attempt (success included) writes three files sharing one attempt id:
/// `ovalbot_page_<id>.txt` (the page the bot really fetched),
/// `ovalbot_response_<id>.txt` (the AJAX response, when a POST happened) and
/// `ovalbot_attempt_<id>.txt` (pack, tickets, price range, outcome).
struct AttemptDump {
    /// Capture directory; `None` disables the dump entirely.
    dir: Option<PathBuf>,
    /// Attempt id shared by the three file names.
    stem: String,
    /// Pack the attempt targeted.
    pack_id: u64,
    /// Tickets of that pack.
    tickets: Vec<String>,
    /// Lower slider bound sent (or read).
    price_min: String,
    /// Upper slider bound sent (or read).
    price_max: String,
}

impl AttemptDump {
    /// Reads `CHECKOUT_DUMP_DIR` and prepares the attempt id.
    ///
    /// # Arguments
    /// * `pack_id` - The pack the attempt targets
    /// * `tickets` - Its ticket ids
    /// * `price_range` - The slider fallback used for the POST
    fn from_env(pack_id: u64, tickets: &[String], price_range: &(String, String)) -> Self {
        let dir = std::env::var("CHECKOUT_DUMP_DIR")
            .ok()
            .map(|dir| dir.trim().to_string())
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            dir,
            stem: nanos.to_string(),
            pack_id,
            tickets: tickets.to_vec(),
            price_min: price_range.0.clone(),
            price_max: price_range.1.clone(),
        }
    }

    /// Writes the attempt files (no-op without `CHECKOUT_DUMP_DIR`).
    ///
    /// # Arguments
    /// * `outcome` - Short verdict (`added`, `rejected`, `missing_context`, …)
    /// * `page` - The fetched page, when available
    /// * `response` - The AJAX response, when a POST happened
    fn finish(&self, outcome: &str, page: Option<&str>, response: Option<&str>) {
        let Some(dir) = &self.dir else {
            return;
        };
        if let Some(page) = page {
            write_dump(dir, "page", &self.stem, page);
        }
        if let Some(response) = response {
            write_dump(dir, "response", &self.stem, response);
        }
        let meta = format!(
            "attempt={}\npack_id={}\ntickets={:?}\nprice_min={}\nprice_max={}\noutcome={outcome}\n",
            self.stem, self.pack_id, self.tickets, self.price_min, self.price_max,
        );
        write_dump(dir, "attempt", &self.stem, &meta);
    }
}

/// Writes one diagnostic file, warning (never failing) on I/O errors.
///
/// # Arguments
/// * `dir` - The capture directory (must exist)
/// * `kind` - File-name kind (`page`, `response`, `attempt`)
/// * `stem` - The attempt id
/// * `body` - The raw payload to save
fn write_dump(dir: &Path, kind: &str, stem: &str, body: &str) {
    let path = dir.join(format!("ovalbot_{kind}_{stem}.txt"));
    if let Err(e) = std::fs::write(&path, body) {
        log::warn!("[CHECKOUT] failed to dump {kind}: {e}");
    }
}

/// Whether two ticket-id lists describe the same pack composition,
/// order-insensitively (the page's ticket map has no stable order).
///
/// # Arguments
/// * `left` - One ticket-id list
/// * `right` - The other ticket-id list
/// # Returns
/// `true` when both hold exactly the same ids
fn same_tickets(left: &[String], right: &[String]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort_unstable();
    right.sort_unstable();
    left == right
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

    // Captured live on 2026-10-31: the same resale right after it sold out —
    // "Aucun billet n'est actuellement disponible", no listing, no price slider,
    // while the add-to-cart form and its settings are still rendered.
    static FIXTURE_OUT_OF_STOCK: LazyLock<String> = LazyLock::new(|| {
        gz_html(include_bytes!(
            "../../app/clubs/larochelle/fixtures/page_out_of_stock.html.gz"
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

    // The price slider is read from the raw HTML too, including the
    // BigPipe/AJAX-escaped markup the site injects after the initial render.
    #[test]
    fn slider_bounds_are_read_from_bigpipe_markup() {
        let escaped = r#"<script>{"data":"\u003Cinput class=\u0022hubber-slider-input\u0022 data-min=\u002249.00\u0022 data-max=\u002249\u0022\u003E"}</script>"#;
        assert_eq!(
            extract_slider_bounds(escaped),
            Some(("49.00".to_string(), "49".to_string()))
        );
        assert_eq!(
            extract_slider_bounds(&FIXTURE_RESALE),
            Some(("44.00".to_string(), "59".to_string()))
        );
        // The sold-out page has neither slider nor escaped chunk.
        assert_eq!(extract_slider_bounds(&FIXTURE_OUT_OF_STOCK), None);
    }

    // The fixture context carries the live values of the capture.
    #[test]
    fn action_context_from_the_fixture() {
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
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
        assert!(extract_action_context("<html><body></body></html>", None).is_none());
        assert!(extract_action_context("Move to waiting queue", None).is_none());
    }

    // On a BigPipe page the settings JSON may carry `ajaxPageState` only (the
    // `edit-add-to-cart` entry arrives with the lazy form): the product URL
    // rebuilds the AJAX endpoint, and the diagnostic tells which piece is
    // missing when the form id itself is absent.
    #[test]
    fn action_context_falls_back_to_the_product_url() {
        let page = r#"<html><body>
<script data-drupal-selector="drupal-settings-json">{"ajaxPageState":{"libraries":"lib-abc","theme":"hubber_reference8"}}</script>
<input type="hidden" name="form_build_id" value="form-bigpipe-7" />
</body></html>"#;
        let context =
            extract_action_context(page, Some("https://shop.example/fr/product/42/revente_j1"))
                .expect("fallback context");
        assert_eq!(
            context.ajax_url,
            "https://shop.example/fr/product/42/revente_j1?ajax_form=1"
        );
        assert_eq!(context.libraries, "lib-abc");
        assert_eq!(context.theme, "hubber_reference8");
        assert_eq!(context.form_build_id, "form-bigpipe-7");
        // Without the form id there is nothing to POST: no context, whatever
        // the fallback URL.
        let no_form = page.replace(
            r#"<input type="hidden" name="form_build_id" value="form-bigpipe-7" />"#,
            "",
        );
        assert!(extract_action_context(&no_form, Some("https://shop.example/x")).is_none());

        let diagnostic = context_diagnostic(page);
        assert!(diagnostic.contains("settings=true"), "{diagnostic}");
        assert!(diagnostic.contains("ajax_entry=false"), "{diagnostic}");
        assert!(diagnostic.contains("form_build_id=true"), "{diagnostic}");
        assert!(diagnostic.contains("slider=false"), "{diagnostic}");
    }

    // urlencode escapes like a form: spaces to `+`, accents percent-encoded.
    #[test]
    fn urlencode_escapes_form_values() {
        assert_eq!(urlencode("user@example.com"), "user%40example.com");
        assert_eq!(urlencode("mon mot de passe"), "mon+mot+de+passe");
        assert_eq!(urlencode("form-abc_1.~"), "form-abc_1.~");
        assert_eq!(urlencode("44.00"), "44.00");
    }

    // The POST body carries the pack, the fresh ids and the replayed state.
    #[test]
    fn add_to_cart_body_contains_every_required_field() {
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
        let body = build_add_to_cart_body(&context, 33850, &["49432".to_string()]);
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

    // A 2-ticket pack (the fixture's pack 33853) is submitted atomically: both
    // ids, comma-joined, exactly like the live payload captured on 2026-11-01.
    #[test]
    fn add_to_cart_body_joins_every_ticket_of_a_pack() {
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
        let body =
            build_add_to_cart_body(&context, 33853, &["49438".to_string(), "49435".to_string()]);
        assert!(body.contains("selected_tickets_33853=49438%2C49435"));
        assert!(body.contains("resale_pack_selected=33853"));
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
    // Success AJAX response shape, mirroring the live capture of 2026-11-01:
    // the pack row gains `added-to-cart` and the cart timeout is refreshed.
    fn add_ok(pack_id: u64) -> String {
        format!(
            r#"[{{"command":"invoke","selector":".js-resale-listing-table tr[data-resale-pack=\"{pack_id}\"] .resale-pack-details","method":"addClass","args":["added-to-cart"]}},{{"command":"redrawListingRow","data":"{pack_id}"}},{{"command":"refreshCartTimeout","data":1498}}]"#
        )
    }

    // A response that carries no success marker (pack gone / rejected).
    const ADD_REJECTED: &str = r#"[{"command":"update_build_id","old":"form-a","new":"form-b"}]"#;
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
        assert!(
            session.jar.path().exists(),
            "cookie jar kept alive while in use"
        );
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

    // Adding a pack inside one session: fresh page → context + presence guard
    // → POST with the pack in the body and the XHR headers.
    #[test]
    fn add_pack_posts_the_add_to_cart_request() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
            page(add_ok(33850), 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );

        let session = ShopSession::login(&base, "user@example.com", "secret").expect("login");
        session
            .add_pack(
                &resale_url,
                33850,
                &["49432".to_string()],
                ("44.00".into(), "59".into()),
            )
            .expect("add-to-cart");

        // The POST landed on the fixture page path and carries the pack.
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

    // On an available resale the listing and the slider are injected later by
    // BigPipe: the fresh page carries no `data-json` and the escaped slider.
    // The bot trusts the plan ids and POSTs with the escaped slider bounds.
    #[test]
    fn add_pack_posts_when_the_page_has_no_listing() {
        let bigpipe_page = r#"<html><body>
<script data-drupal-selector="drupal-settings-json">{"ajaxPageState":{"theme":"hubber_reference8","libraries":"lib-abc"},"ajax":{"edit-add-to-cart":{"url":"/fr/product/1360/revente?ajax_form=1"}}}</script>
<input type="hidden" name="form_build_id" value="form-bigpipe-1" />
<script>{"data":"\u003Cinput class=\u0022hubber-slider-input\u0022 data-min=\u002249.00\u0022 data-max=\u002249\u0022\u003E"}</script>
</body></html>"#;

        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(bigpipe_page, 200),
            page(add_ok(34720), 200),
        ]);
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        session
            .add_pack(
                &format!("{base}/fr/product/1360/revente"),
                34720,
                &["50656".to_string()],
                ("49.00".into(), "49".into()),
            )
            .expect("add-to-cart");

        let log = requests.lock().expect("log");
        let post = log
            .iter()
            .find(|r| r.contains("POST /fr/product/1360"))
            .expect("add-to-cart POST recorded");
        assert!(post.contains("selected_tickets_34720=50656"));
        assert!(post.contains("price_slider=49.00%2C49"));
        assert_eq!(log.len(), 6);
    }

    // A response without any success marker means the pack was gone: SeatGone,
    // never a claim on a pack that never reached the cart.
    #[test]
    fn add_pack_reports_a_rejected_add_as_seat_gone() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
            page(ADD_REJECTED, 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        assert_eq!(
            session
                .add_pack(
                    &resale_url,
                    33850,
                    &["49432".to_string()],
                    ("44.00".into(), "59".into())
                )
                .expect_err("must fail"),
            CheckoutError::SeatGone
        );
        assert_eq!(requests.lock().expect("log").len(), 6);
    }

    // The live success response (2026-11-01) carries all three markers; a
    // response for another pack or with none of them is a rejection.
    #[test]
    fn pack_added_reads_the_live_success_response() {
        let live = r#"[{"command":"update_build_id","old":"form-a","new":"form-b"},{"command":"insert","method":"replaceWith","selector":".js-show-temp-cart-wrapper .ticket-form-footer-container","data":"\u003Cdiv class=\u0022total-price\u0022\u003E49\u00a0\u20ac\u003C\/div\u003E"},{"command":"invoke","selector":".js-resale-listing-table tr[data-resale-pack=\u002234720\u0022] .resale-pack-details","method":"addClass","args":["added-to-cart"]},{"command":"redrawListingRow","data":"34720"},{"command":"refreshCartTimeout","data":1498}]"#;
        assert!(pack_added(live, 34720));
        assert!(!pack_added(live, 11111));
        assert!(!pack_added(ADD_REJECTED, 34720));
        assert!(!pack_added("[]", 34720));
    }

    // The diagnostic capture writes one file per kind sharing the attempt id,
    // and the metadata carries the pack, tickets, slider bounds and outcome.
    #[test]
    fn attempt_dump_writes_page_response_and_metadata() {
        let dir = std::env::temp_dir().join(format!("ovalbot-dump-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let dump = AttemptDump {
            dir: Some(dir.clone()),
            stem: "42".to_string(),
            pack_id: 900,
            tickets: vec!["a".to_string(), "b".to_string()],
            price_min: "49.00".to_string(),
            price_max: "49".to_string(),
        };
        dump.finish("added", Some("<html/>"), Some("[]"));

        assert_eq!(
            std::fs::read_to_string(dir.join("ovalbot_page_42.txt")).expect("page"),
            "<html/>"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("ovalbot_response_42.txt")).expect("response"),
            "[]"
        );
        let meta = std::fs::read_to_string(dir.join("ovalbot_attempt_42.txt")).expect("meta");
        assert!(meta.contains("pack_id=900"), "{meta}");
        assert!(meta.contains("tickets=[\"a\", \"b\"]"), "{meta}");
        assert!(meta.contains("outcome=added"), "{meta}");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    // Without CHECKOUT_DUMP_DIR the capture is a no-op (no panic, no file).
    #[test]
    fn attempt_dump_is_a_noop_without_a_directory() {
        let dump = AttemptDump {
            dir: None,
            stem: "43".to_string(),
            pack_id: 900,
            tickets: vec!["a".to_string()],
            price_min: "49.00".to_string(),
            price_max: "49".to_string(),
        };
        dump.finish("rejected", Some("<html/>"), None);
    }

    // A stale-session rejection body is mapped to ExpiredForm.
    #[test]
    fn add_pack_reports_the_expired_form_rejection() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
            page(ADD_EXPIRED, 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        assert_eq!(
            session
                .add_pack(
                    &resale_url,
                    33850,
                    &["49432".to_string()],
                    ("44.00".into(), "59".into())
                )
                .expect_err("must fail"),
            CheckoutError::ExpiredForm
        );
        assert_eq!(requests.lock().expect("log").len(), 6);
    }

    // A seat that vanished from the fresh page is reported as SeatGone (no
    // add-to-cart POST is sent).
    #[test]
    fn add_pack_reports_a_seat_gone() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        assert_eq!(
            session
                .add_pack(
                    &resale_url,
                    999,
                    &["0".to_string()],
                    ("44.00".into(), "59".into())
                )
                .expect_err("must fail"),
            CheckoutError::SeatGone
        );
        // 4 login requests + 1 page fetch; no POST.
        assert_eq!(requests.lock().expect("log").len(), 5);
    }

    // A sold-out resale page no longer lists packs: the bot trusts the plan ids
    // and POSTs, and the AJAX verdict (no success marker) maps to SeatGone.
    #[test]
    fn add_pack_reads_a_sold_out_page_as_seat_gone() {
        assert!(
            extract_action_context(&FIXTURE_OUT_OF_STOCK, None).is_some(),
            "the sold-out page still carries the add-to-cart form and settings"
        );
        assert!(
            parse_seat::parse_pack_ticket_ids(&FIXTURE_OUT_OF_STOCK).is_empty(),
            "the sold-out fixture lists no pack"
        );

        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_OUT_OF_STOCK, 200),
            page(ADD_REJECTED, 200),
        ]);
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        assert_eq!(
            session
                .add_pack(
                    &format!("{base}/fr/product/1360/revente"),
                    34700,
                    &["50624".to_string()],
                    ("44.00".into(), "44".into()),
                )
                .expect_err("must fail"),
            CheckoutError::SeatGone
        );
        // 4 login requests + 1 page fetch + 1 rejected POST.
        assert_eq!(requests.lock().expect("log").len(), 6);
    }

    // A pack re-minted with another composition (one ticket sold inside a duo,
    // ids changed) is stale: SeatGone, never a truncated POST.
    #[test]
    fn add_pack_reports_a_repacked_pack_as_seat_gone() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(&*FIXTURE_RESALE, 200),
        ]);
        let context = extract_action_context(&FIXTURE_RESALE, None).expect("context");
        let resale_url = format!(
            "{base}{}",
            context.ajax_url.split('?').next().expect("path")
        );
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        // Pack 33853 lists 49435 and 49438: asking for one of them only is a
        // different composition, so the plan is stale.
        assert_eq!(
            session
                .add_pack(
                    &resale_url,
                    33853,
                    &["49435".to_string()],
                    ("59.00".into(), "59".into())
                )
                .expect_err("must fail"),
            CheckoutError::SeatGone
        );
        assert_eq!(requests.lock().expect("log").len(), 5);
    }

    // The waiting queue during the pack addition is reported distinctly.
    #[test]
    fn add_pack_reports_the_waiting_queue() {
        let (base, requests) = serve_mock(vec![
            page(LOGIN_PAGE, 200),
            page(STEP2_PAGE, 200),
            page("<html>connecté</html>", 200),
            page(BASKET_AUTHENTICATED, 200),
            page(QUEUED, 307),
        ]);
        let session = ShopSession::login(&base, "a@b.c", "x").expect("login");
        assert_eq!(
            session
                .add_pack(
                    &format!("{base}/fr/product/1/resale"),
                    33850,
                    &["49432".to_string()],
                    ("44.00".into(), "59".into()),
                )
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
            let jar = session.jar.path().clone();
            assert!(jar.exists());
            drop(session);
            jar
        };
        assert!(!jar.exists(), "cookie jar deleted on drop");
    }

    // The guard removes the temp file on drop: every failed login relies on it
    // (no session is built, so nothing else would clean up).
    #[test]
    fn jar_guard_deletes_the_file_on_drop() {
        let jar = SessionJar::fresh();
        std::fs::write(jar.path(), b"cookies").expect("write jar");
        assert!(jar.path().exists());
        let path = jar.path().clone();
        drop(jar);
        assert!(!path.exists(), "cookie jar deleted on drop");
    }
}
