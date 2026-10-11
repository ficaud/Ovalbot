use crate::error::FetchError;
use reqwest::blocking::Client;
use reqwest::header;
use reqwest::StatusCode;
use std::time::Duration;

pub struct WebPageFetcher;

impl WebPageFetcher {
    pub fn fetch(&self, url: &str) -> Result<String, FetchError> {
        fetch_html(url)
    }
}

fn fetch_html(url: &str) -> Result<String, FetchError> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()?;
    let response = client
        .get(url)
        .header(
            header::USER_AGENT,
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
        )
        .send()?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(FetchError::NotFound);
    }
    Ok(response.text()?)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn serve_once(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, handle)
    }

    /// Fetches a page from a local mock server and returns its HTML body verbatim.
    #[test]
    fn fetcher_returns_page_body() {
        let body = "<html><body>Hello from the mock server</body></html>";
        let (url, handle) = serve_once("200 OK", body);

        let html = fetch_html(&url).unwrap();

        handle.join().unwrap();
        assert_eq!(html, body);
    }

    /// Sends a browser-like `User-Agent` header on every request, which the site
    /// requires to serve HTML.
    #[test]
    fn fetcher_sends_user_agent() {
        let (url, handle) = serve_once("200 OK", "<html></html>");

        fetch_html(&url).unwrap();

        let request = handle.join().unwrap();
        assert!(request.contains("Mozilla/5.0"));
    }

    /// Maps an HTTP 404 response to `FetchError::NotFound`, which callers use to
    /// stop iterating over calendar pages.
    #[test]
    fn fetcher_returns_not_found_on_404() {
        let (url, handle) = serve_once("404 Not Found", "<html>404</html>");

        let result = fetch_html(&url);

        handle.join().unwrap();
        assert!(matches!(result, Err(FetchError::NotFound)));
    }
}
