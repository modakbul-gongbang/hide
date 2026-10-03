//! Static resources declared by a checkout HTML preview. The preview is not
//! a general-purpose file server: undeclared checkout files stay private.

use std::collections::HashMap;
use std::path::Path;

use percent_encoding::percent_decode_str;

pub const MAX_ASSETS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Asset {
    Css,
    Script,
    Image,
    Font,
}

pub fn html_assets(base: &str, bytes: &[u8]) -> HashMap<String, Asset> {
    let html = String::from_utf8_lossy(bytes);
    let text = html.as_bytes();
    let mut assets = HashMap::new();
    let mut at = 0;
    while at < text.len() && assets.len() <= MAX_ASSETS {
        if text[at] != b'<' {
            at += 1;
            continue;
        }
        if text[at..].starts_with(b"<!--") {
            at = find_bytes(&text[at + 4..], b"-->").map_or(text.len(), |end| at + 4 + end + 3);
            continue;
        }
        let start = at + 1;
        let mut name_end = start;
        while name_end < text.len() && text[name_end].is_ascii_alphabetic() {
            name_end += 1;
        }
        if name_end == start {
            at += 1;
            continue;
        }
        let tag = text[start..name_end].to_ascii_lowercase();
        let mut end = name_end;
        let mut quote = None;
        while end < text.len() {
            let byte = text[end];
            if let Some(expected) = quote {
                if byte == expected {
                    quote = None;
                }
            } else if byte == b'\'' || byte == b'"' {
                quote = Some(byte);
            } else if byte == b'>' {
                break;
            }
            end += 1;
        }
        if end == text.len() {
            break;
        }
        if matches!(tag.as_slice(), b"script" | b"link" | b"img") {
            let attributes = attributes(&text[name_end..end]);
            let reference = match tag.as_slice() {
                b"script" => attributes.get("src").map(|value| (value, Asset::Script)),
                b"link"
                    if attributes.get("rel").is_some_and(|value| {
                        value
                            .split_ascii_whitespace()
                            .any(|part| part.eq_ignore_ascii_case("stylesheet"))
                    }) =>
                {
                    attributes.get("href").map(|value| (value, Asset::Css))
                }
                b"img" => attributes.get("src").map(|value| (value, Asset::Image)),
                _ => None,
            };
            if let Some((reference, kind)) = reference
                && let Some(path) = relative_asset(base, reference, kind)
            {
                assets.insert(path, kind);
            }
        }
        at = end + 1;
        if tag == b"script" || tag == b"style" {
            let closing = if tag == b"script" {
                b"</script".as_slice()
            } else {
                b"</style".as_slice()
            };
            at = find_case_insensitive(&text[at..], closing).map_or(text.len(), |next| at + next);
        }
    }
    assets
}

pub fn css_assets(base: &str, bytes: &[u8]) -> HashMap<String, Asset> {
    let text = String::from_utf8_lossy(bytes);
    let input = text.as_bytes();
    let mut assets = HashMap::new();
    let mut at = 0;
    while at < input.len() && assets.len() <= MAX_ASSETS {
        if input[at..].starts_with(b"/*") {
            at = find_bytes(&input[at + 2..], b"*/").map_or(input.len(), |end| at + 2 + end + 2);
            continue;
        }
        if !input[at..]
            .get(..4)
            .is_some_and(|part| part.eq_ignore_ascii_case(b"url("))
        {
            at += 1;
            continue;
        }
        let start = at + 4;
        let Some(end) = input[start..].iter().position(|byte| *byte == b')') else {
            break;
        };
        let value = text[start..start + end]
            .trim()
            .trim_matches(['\'', '"'])
            .trim();
        for kind in [Asset::Image, Asset::Font] {
            if let Some(path) = relative_asset(base, value, kind) {
                assets.insert(path, kind);
                break;
            }
        }
        at = start + end + 1;
    }
    assets
}

fn attributes(input: &[u8]) -> HashMap<String, String> {
    let mut found = HashMap::new();
    let mut at = 0;
    while at < input.len() {
        while at < input.len() && !input[at].is_ascii_alphabetic() {
            at += 1;
        }
        let start = at;
        while at < input.len()
            && (input[at].is_ascii_alphanumeric() || matches!(input[at], b'-' | b'_'))
        {
            at += 1;
        }
        if at == start {
            break;
        }
        let name = String::from_utf8_lossy(&input[start..at]).to_ascii_lowercase();
        while at < input.len() && input[at].is_ascii_whitespace() {
            at += 1;
        }
        if input.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        while at < input.len() && input[at].is_ascii_whitespace() {
            at += 1;
        }
        let quote = input
            .get(at)
            .copied()
            .filter(|byte| *byte == b'\'' || *byte == b'"');
        if quote.is_some() {
            at += 1;
        }
        let value_start = at;
        while at < input.len()
            && if let Some(quote) = quote {
                input[at] != quote
            } else {
                !input[at].is_ascii_whitespace()
            }
        {
            at += 1;
        }
        let value = String::from_utf8_lossy(&input[value_start..at]).into_owned();
        if quote.is_some() && at < input.len() {
            at += 1;
        }
        found.entry(name).or_insert(value);
    }
    found
}

fn relative_asset(base: &str, reference: &str, kind: Asset) -> Option<String> {
    let raw = reference.split(['?', '#']).next()?;
    if raw.is_empty() || raw.starts_with(['/', '\\']) || raw.contains(':') {
        return None;
    }
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    let mut parts = base.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ if part.starts_with('.') || part.contains(['\\', '\0']) => return None,
            _ => parts.push(part),
        }
    }
    let path = parts.join("/");
    hide_platform::path::RelPath::parse(&path).ok()?;
    let ext = Path::new(&path).extension()?.to_str()?.to_ascii_lowercase();
    let valid = match kind {
        Asset::Css => ext == "css",
        Asset::Script => matches!(ext.as_str(), "js" | "mjs"),
        Asset::Image => matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "ico"
        ),
        Asset::Font => matches!(ext.as_str(), "woff" | "woff2" | "ttf" | "otf"),
    };
    valid.then_some(path)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_declared_static_assets_enter_the_preview() {
        let html = br#"<!-- <script src='.env'></script> -->
            <link href='../style.css' rel='stylesheet'>
            <img src='images/a.png'><script src='app.js'></script>
            <script>const fake = "<img src='secret.json'>";</script>
            <a href='notes.txt'>notes</a>"#;
        let assets = html_assets("pages/preview.html", html);
        assert_eq!(assets.get("style.css"), Some(&Asset::Css));
        assert_eq!(assets.get("pages/images/a.png"), Some(&Asset::Image));
        assert_eq!(assets.get("pages/app.js"), Some(&Asset::Script));
        assert!(!assets.contains_key("pages/notes.txt"));
        assert!(!assets.contains_key("pages/secret.json"));
        assert!(!assets.contains_key("pages/.env"));
    }
}
