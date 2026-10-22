//! HTML fetching abstraction.
//!
//! The high-level API is [`Fetched`] + [`FetchHtml::get_html_tracked`]: it
//! carries the body **and** the final URL after redirects, which the caller
//! needs to detect the billetterie waiting queue (the shop redirects there
//! under load). [`FetchHtml::get_html`] stays the plain-body variant.

/// A fetched page: its body plus the final URL after following redirects.
pub struct Fetched {
    /// The URL the request actually landed on (curl's effective URL).
    pub final_url: String,
    /// The response body as a string.
    pub body: String,
}

/// Trait exposing the parsing layer's only I/O need: GET an HTML page.
pub trait FetchHtml {
    /// Fetches the HTML content of the given URL.
    ///
    /// # Arguments
    /// * `url` - The URL to fetch
    /// # Returns
    /// The HTML content as a `String`, or an error message if the fetch fails
    fn get_html(&self, url: &str) -> Result<String, String>;

    /// Fetches a page as [`Fetched`] (body + effective URL after redirects).
    ///
    /// The default implementation is the plain [`get_html`](Self::get_html)
    /// with the requested URL as `final_url` — only the real HTTP client
    /// tracks redirects. Doubles in tests need no tracking.
    ///
    /// # Arguments
    /// * `url` - The URL to fetch
    /// # Returns
    /// The [`Fetched`] page, or an error message if the fetch fails
    fn get_html_tracked(&self, url: &str) -> Result<Fetched, String> {
        self.get_html(url).map(|body| Fetched {
            final_url: url.to_owned(),
            body,
        })
    }
}
