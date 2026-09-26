//! The `file:` URLs a browser display loads (issue 155). A client names a
//! local file through one, so the daemon checks the path it carries like
//! any other path a client sends: it decodes it, runs it through the
//! boundary, and writes the checked path back in the one spelling the web
//! shell and the CLI also produce.

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

/// WHATWG's path percent-encode set, which Chromium writes a file URL in,
/// plus `%`, so a decoded path encodes back to the URL it came from.
const PATH_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// Whether `url` has the `file:` scheme, in any case.
pub fn is_file_url(url: &str) -> bool {
    url.get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
}

/// The absolute path a `file:` URL names on this machine and the query or
/// fragment after it. None when the URL names another host, has no absolute
/// path, or does not decode to UTF-8 without a NUL.
pub fn file_path(url: &str) -> Option<(String, &str)> {
    if !is_file_url(url) {
        return None;
    }
    let rest = url[5..].strip_prefix("//")?;
    let (host, located) = rest.split_at(rest.find('/')?);
    if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let (path, suffix) = located.split_at(located.find(['?', '#']).unwrap_or(located.len()));
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    if decoded.contains('\0') {
        return None;
    }
    Some((decoded.into_owned(), suffix))
}

/// The `file:` URL of an absolute path, with `suffix` (a query or fragment)
/// kept as written.
pub fn file_url(path: &str, suffix: &str) -> String {
    format!("file://{}{suffix}", utf8_percent_encode(path, PATH_SET))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_round_trips_through_its_url() {
        for path in [
            "/a/b.html",
            "/문서/보고서 1.html",
            "/a/100%/#x?.html",
            "/a/{b}`c\"<d>",
        ] {
            let url = file_url(path, "");
            assert_eq!(file_path(&url), Some((path.to_owned(), "")), "{url}");
        }
        assert_eq!(
            file_url("/문서/a b.html", "#top"),
            "file:///%EB%AC%B8%EC%84%9C/a%20b.html#top"
        );
    }

    #[test]
    fn the_query_and_fragment_stay_outside_the_path() {
        assert_eq!(
            file_path("file:///a/b.html?x=1#top"),
            Some(("/a/b.html".to_owned(), "?x=1#top"))
        );
        assert_eq!(
            file_path("FILE://localhost/a%20b"),
            Some(("/a b".to_owned(), ""))
        );
    }

    #[test]
    fn another_host_or_a_bad_path_is_not_a_local_file() {
        for url in [
            "file://server/share/a",
            "file:/a/b",
            "file:a",
            "file:///a%00b",
            "file:///%FF",
            "https://a/b",
        ] {
            assert_eq!(file_path(url), None, "{url}");
        }
    }
}
