use crate::controller::html_extract::{FetchHtml, Fetched};
use curl::easy::Easy;
use std::time::Duration;

/// WebClient is a struct that implements the `FetchHtml` trait, allowing it to fetch HTML content from a URL.
pub struct WebClient;

/// WebClient implementation: a plain GET client with direct connections.
/// The shop login / add-to-cart flows and the proxy pool were removed
/// (pure scanning only, no proxy management).
impl WebClient {
    /// Creates the client (direct connections only).
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebClient {
    /// Same as [`WebClient::new`] — a direct-connection GET client.
    fn default() -> Self {
        Self::new()
    }
}

impl FetchHtml for WebClient {
    /// Performs the GET and maps the error into the trait's string form.
    fn get_html(&self, url: &str) -> Result<String, String> {
        fetch_html(url)
            .map(|(_, body)| body)
            .map_err(|e| e.to_string())
    }

    /// Performs the GET and returns the body **with the effective URL**, so the
    /// caller can detect the billetterie waiting-queue redirect.
    fn get_html_tracked(&self, url: &str) -> Result<Fetched, String> {
        fetch_html(url)
            .map(|(final_url, body)| Fetched { final_url, body })
            .map_err(|e| e.to_string())
    }
}

/// Fetches HTML content from the given URL over a direct connection.
///
/// # Arguments
/// * `url` - The URL from which to fetch HTML content
/// # Returns
/// The effective URL after following redirects and the fetched HTML content,
/// or an error if the fetch operation fails
fn fetch_html(url: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let mut easy = Easy::new();
    easy.url(url)?;
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
        let client = WebClient::new();
        assert_eq!(
            client.get_html(&url).unwrap(),
            "<html>bonjour du mock</html>"
        );
    }

    // Without redirect, the tracked fetch reports the requested URL as final.
    #[test]
    fn get_html_tracked_reports_the_requested_url_without_redirect() {
        let url = serve("<html>page</html>", 1);
        let client = WebClient::new();
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

        let client = WebClient::new();
        assert!(
            client
                .get_html(&format!("http://127.0.0.1:{port}/"))
                .is_err()
        );
    }
}
