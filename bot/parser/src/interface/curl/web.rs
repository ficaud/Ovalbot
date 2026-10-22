use super::proxy::{ProxyMode, retry_with_proxy_mode};
use crate::controller::html_extract::{FetchHtml, Fetched};
use curl::easy::Easy;
use std::time::Duration;

/// WebClient is a struct that implements the `FetchHtml` trait, allowing it to fetch HTML content from a URL.
pub struct WebClient {
    pub proxy_mode: ProxyMode,
}

/// WebClient implementation: a plain GET client with proxy rotation/failover.
/// The shop login / add-to-cart flows were removed (pure scanning only).
impl WebClient {
    /// Creates the client with the given proxy strategy (direct by default).
    pub fn new(proxy_mode: ProxyMode) -> Self {
        Self { proxy_mode }
    }

    /// Swaps the proxy strategy at runtime (used by the proxy-mode tests and
    /// by failover logic that downgrades to direct connections).
    pub fn set_proxy_mode(&mut self, proxy_mode: ProxyMode) {
        self.proxy_mode = proxy_mode;
    }
}

impl FetchHtml for WebClient {
    /// Performs the GET and maps the error into the trait's string form.
    fn get_html(&self, url: &str) -> Result<String, String> {
        fetch_html(url, self.proxy_mode)
            .map(|(_, body)| body)
            .map_err(|e| e.to_string())
    }

    /// Performs the GET and returns the body **with the effective URL**, so the
    /// caller can detect the billetterie waiting-queue redirect.
    fn get_html_tracked(&self, url: &str) -> Result<Fetched, String> {
        fetch_html(url, self.proxy_mode)
            .map(|(final_url, body)| Fetched { final_url, body })
            .map_err(|e| e.to_string())
    }
}

/// Fetches HTML content from the given URL.
///
/// # Arguments
/// * `url` - The URL from which to fetch HTML content
/// * `proxy_mode` - How to select proxies when retrying
/// # Returns
/// The effective URL after following redirects and the fetched HTML content,
/// or an error if the fetch operation fails
fn fetch_html(
    url: &str,
    proxy_mode: ProxyMode,
) -> Result<(String, String), Box<dyn std::error::Error>> {
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
            let final_url = match easy.effective_url()? {
                Some(u) => u.to_owned(),
                None => url.to_owned(),
            };
            Ok((final_url, String::from_utf8(html)?))
        },
        proxy_mode,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// Binds a listener on an ephemeral port and serves a canned HTTP
    /// response for the first `responses` connections.
    fn serve(body: &'static str, responses: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        thread::spawn(move || {
            for stream in listener.incoming().take(responses) {
                let mut stream = stream.expect("accept");
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/page")
    }

    #[test]
    fn get_html_returns_the_server_body() {
        let url = serve("<html>bonjour du mock</html>", 1);
        let client = WebClient::new(ProxyMode::Disabled);
        assert_eq!(
            client.get_html(&url).unwrap(),
            "<html>bonjour du mock</html>"
        );
    }

    // Without redirect, the tracked fetch reports the requested URL as final.
    #[test]
    fn get_html_tracked_reports_the_requested_url_without_redirect() {
        let url = serve("<html>page</html>", 1);
        let client = WebClient::new(ProxyMode::Disabled);
        let page = client.get_html_tracked(&url).unwrap();
        assert_eq!(page.final_url, url);
        assert_eq!(page.body, "<html>page</html>");
    }

    #[test]
    fn get_html_fails_when_the_connection_is_refused() {
        // Bind and drop to grab a port with nothing listening on it.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        drop(listener);

        let client = WebClient::new(ProxyMode::Disabled);
        assert!(
            client
                .get_html(&format!("http://127.0.0.1:{port}/"))
                .is_err()
        );
    }

    #[test]
    fn set_proxy_mode_updates_the_client() {
        let mut client = WebClient::new(ProxyMode::Disabled);
        assert_eq!(client.proxy_mode, ProxyMode::Disabled);
        client.set_proxy_mode(ProxyMode::Rotating);
        assert_eq!(client.proxy_mode, ProxyMode::Rotating);
    }
}
