use super::proxy::{ProxyMode, retry_with_proxy_mode};
use crate::controller::html_extract::FetchHtml;
use curl::easy::Easy;
use std::time::Duration;

/// WebClient is a struct that implements the `FetchHtml` trait, allowing it to fetch HTML content from a URL.
pub struct WebClient {
    pub proxy_mode: ProxyMode,
}

/// WebClient implementation: a plain GET client with proxy rotation/failover.
/// The shop login / add-to-cart flows were removed (pure scanning only).
impl WebClient {
    pub fn new(proxy_mode: ProxyMode) -> Self {
        Self { proxy_mode }
    }

    pub fn set_proxy_mode(&mut self, proxy_mode: ProxyMode) {
        self.proxy_mode = proxy_mode;
    }
}

impl FetchHtml for WebClient {
    fn get_html(&self, url: &str) -> Result<String, String> {
        fetch_html(url, self.proxy_mode).map_err(|e| e.to_string())
    }
}

/// Fetches HTML content from the given URL.
///
/// # Arguments
/// * `url` - The URL from which to fetch HTML content
/// * `proxy_mode` - How to select proxies when retrying
/// # Returns
/// A `Result` containing the fetched HTML content as a `String`, or an error if the fetch operation fails
fn fetch_html(url: &str, proxy_mode: ProxyMode) -> Result<String, Box<dyn std::error::Error>> {
    retry_with_proxy_mode(
        |proxy| {
            let mut easy = Easy::new();
            easy.url(url)?;
            if let Some(p) = proxy {
                easy.proxy(p)?;
            }
            easy.connect_timeout(Duration::from_secs(5))?;
            easy.timeout(Duration::from_secs(30))?;
            easy.follow_location(true)?;
            let mut html = Vec::new();
            {
                let mut transfer = easy.transfer();
                transfer.write_function(|data| {
                    html.extend_from_slice(data);
                    Ok(data.len())
                })?;
                transfer.perform()?;
            }
            Ok(String::from_utf8(html)?)
        },
        proxy_mode,
    )
}
