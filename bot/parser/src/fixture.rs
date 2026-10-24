//! Test-only support for the gzipped HTML fixtures. Fixtures are committed as
//! `.html.gz` to keep the repository lean (live-captured pages are bulky); the
//! decompressed bytes are byte-for-byte identical to the original capture, so
//! every exact-value assertion the tests make still holds.

use flate2::read::GzDecoder;
use std::io::Read as _;

/// Decompresses a gzipped fixture embedded with `include_bytes!` into the
/// original UTF-8 HTML string.
///
/// # Arguments
///
/// * `gz` — gzip stream of the fixture (typically `include_bytes!("X.html.gz")`).
///
/// # Returns
///
/// The decompressed HTML as UTF-8, identical to what `include_str!` used to
/// embed before the gzip compression.
///
/// # Panics
///
/// Panics if the stream is not valid gzip or not valid UTF-8. Fixtures are
/// frozen assets captured live, so this can only mean a corrupted fixture.
pub(crate) fn gz_html(gz: &[u8]) -> String {
    let mut html = String::new();
    GzDecoder::new(gz)
        .read_to_string(&mut html)
        .expect("fixture must be valid gzip and valid UTF-8");
    html
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    /// Round-trip: a known gzip stream must decode to its original text.
    #[test]
    fn decodes_gzip_to_the_original_text() {
        let source = "<html><body>fixture &amp; noise</body></html>";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(source.as_bytes()).unwrap();
        let gz = encoder.finish().unwrap();

        assert_eq!(gz_html(&gz), source);
    }

    /// A non-gzip stream must panic instead of silently producing garbage.
    #[test]
    #[should_panic(expected = "fixture must be valid gzip")]
    fn panics_on_a_non_gzip_stream() {
        gz_html(b"not gzip");
    }
}
