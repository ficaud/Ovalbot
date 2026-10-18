/// This module defines the `FetchHtml` trait, which abstracts HTTP GET requests to fetch HTML content from a URL.
pub trait FetchHtml {
    /// Fetches the HTML content of the given URL.
    ///
    /// # Arguments
    /// * `url` - The URL to fetch
    /// # Returns
    /// The HTML content as a `String`, or an error message if the fetch fails
    fn get_html(&self, url: &str) -> Result<String, String>;
}
