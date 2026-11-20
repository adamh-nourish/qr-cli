//! Web links.

use std::fmt;

use super::PayloadError;

const FIELD: &str = "URL";

/// Characters RFC 3986 never allows raw in a URI. Non-ASCII text is
/// allowed (an IRI, RFC 3987), since phones open those as typed.
const FORBIDDEN: &[char] = &['"', '<', '>', '\\', '^', '`', '{', '|', '}'];

/// Schemes whose URIs must name a host.
const NEEDS_HOST: &[&str] = &["http", "https", "ftp", "ftps", "ws", "wss"];

/// Schemes that another subcommand builds properly, with that subcommand.
const OTHER_SUBCOMMAND: &[(&str, &str)] = &[
    (
        "mailto",
        "use qr email for mailto: links, for example: qr email ada@example.com",
    ),
    (
        "tel",
        "use qr tel for tel: links, for example: qr tel '+44 20 7946 0000'",
    ),
    (
        "sms",
        "use qr sms for text messages, for example: qr sms '+44 7700 900123' 'hello'",
    ),
    (
        "smsto",
        "use qr sms for text messages, for example: qr sms '+44 7700 900123' 'hello'",
    ),
    (
        "geo",
        "use qr geo for locations, for example: qr geo 51.5007 -0.1246",
    ),
    (
        "wifi",
        "use qr wifi for WiFi networks (see 'qr wifi --help')",
    ),
    (
        "mecard",
        "use qr mecard for contact cards (see 'qr mecard --help')",
    ),
];

/// Schemes that run code or embed a document instead of linking to one.
/// Phones handle them inconsistently at best, so they are refused.
const REFUSED: &[&str] = &["javascript", "vbscript", "data"];

/// A validated link, such as `https://example.com/a?b=c`.
///
/// Text that starts with a scheme (`name:` where the colon is not followed
/// by a digit, so `localhost:8080` has none) keeps it; anything else gets
/// `https://`. The scheme is lower-cased; the rest is kept exactly as
/// given. Web schemes (`http`, `https`, `ftp`, ...) must be followed by
/// `//` and a valid host. `mailto:`, `tel:`, `sms:`, `geo:` and `wifi:`
/// are refused in favour of the subcommand that builds them, and
/// `javascript:`, `vbscript:` and `data:` are refused outright.
///
/// ```
/// use qr_core::payload::Url;
/// let url = Url::new("example.com/docs")?;
/// assert_eq!(url.to_string(), "https://example.com/docs");
/// assert_eq!(Url::new("localhost:8080")?.as_str(), "https://localhost:8080");
/// assert_eq!(Url::new("urn:isbn:0451450523")?.as_str(), "urn:isbn:0451450523");
/// assert!(Url::new("https://exa mple.com").is_err());
/// assert!(Url::new("mailto:ada@example.com").is_err());
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Url(String);

impl Url {
    /// Validates `link`, adding `https://` when it has no scheme.
    ///
    /// # Errors
    ///
    /// [`PayloadError`] when the link is empty, holds whitespace, a control
    /// character or a character URIs never allow raw (such as `"`, `<` or a
    /// backtick), has a malformed scheme, lacks a valid host for a web
    /// scheme, or uses a scheme that is refused (see the type
    /// documentation).
    pub fn new(link: &str) -> Result<Self, PayloadError> {
        Self::with_field(FIELD, link)
    }

    /// As [`Url::new`], naming `field` in errors.
    pub(crate) fn with_field(field: &'static str, link: &str) -> Result<Self, PayloadError> {
        if link.is_empty() {
            return Err(PayloadError::Empty { field });
        }
        if let Some(character) = link.chars().find(|c| c.is_control()) {
            return Err(PayloadError::ControlCharacter { field, character });
        }
        if let Some(character) = link.chars().find(|&c| crate::sanitize::is_bidi_format(c)) {
            return Err(PayloadError::BidiCharacter { field, character });
        }
        if link.chars().any(char::is_whitespace) {
            return Err(PayloadError::invalid_with_hint(
                field,
                link,
                "contains whitespace",
                "write a space as %20",
            ));
        }
        if link.contains(FORBIDDEN) {
            return Err(PayloadError::invalid_with_hint(
                field,
                link,
                "contains a character URLs cannot hold raw",
                "percent-encode it, for example '<' as %3C",
            ));
        }
        let Some((scheme, rest)) = split_scheme(link) else {
            check_authority(field, link, link)?;
            return Ok(Self(format!("https://{link}")));
        };
        let scheme = scheme.to_ascii_lowercase();
        if !is_scheme(&scheme) {
            return Err(PayloadError::invalid(
                field,
                link,
                "the scheme must be a letter followed by letters, digits, '+', '-' or '.'",
            ));
        }
        if let Some((_, hint)) = OTHER_SUBCOMMAND.iter().find(|(s, _)| *s == scheme) {
            return Err(PayloadError::invalid_with_hint(
                field,
                link,
                "this kind of link has its own subcommand",
                hint,
            ));
        }
        if REFUSED.contains(&scheme.as_str()) {
            return Err(PayloadError::invalid_with_hint(
                field,
                link,
                "script and data: links are not allowed",
                "to encode the text exactly as given, pass it to qr itself: qr -- 'TEXT'",
            ));
        }
        if NEEDS_HOST.contains(&scheme.as_str()) {
            let Some(authority) = rest.strip_prefix("//") else {
                return Err(PayloadError::invalid_with_hint(
                    field,
                    link,
                    "a web link needs // after the scheme",
                    "write it as scheme://host/path, for example https://example.com",
                ));
            };
            check_authority(field, link, authority)?;
        }
        Ok(Self(format!("{scheme}:{rest}")))
    }

    /// The link as it is encoded.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Splits an explicit scheme off `link`, returning it and the text after
/// its colon: the text before a `://`, or before a `:` that is not followed
/// by a digit (`localhost:8080` is a host and a port) unless it is one of
/// the schemes refused below (`geo:51.5,0`). A scheme before
/// `://` is not validated here, so that a malformed one is reported rather
/// than read as a host.
fn split_scheme(link: &str) -> Option<(&str, &str)> {
    if let Some((scheme, _)) = link.split_once("://") {
        if !scheme.contains([':', '/', '?', '#', '@', '[']) {
            return Some((scheme, &link[scheme.len() + 1..]));
        }
    }
    let (scheme, rest) = link.split_once(':')?;
    let starts_with_digit = rest.bytes().next().is_some_and(|b| b.is_ascii_digit());
    // `geo:51.5,0` and `tel:0123` are still schemes: no host is named so.
    let known = OTHER_SUBCOMMAND
        .iter()
        .map(|(s, _)| *s)
        .chain(REFUSED.iter().copied())
        .any(|s| s.eq_ignore_ascii_case(scheme));
    (is_scheme(scheme) && !rest.is_empty() && (known || !starts_with_digit))
        .then_some((scheme, rest))
}

/// RFC 3986 section 3.1: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// Checks the authority (`[userinfo@]host[:port]`) at the start of `rest`.
fn check_authority(field: &'static str, link: &str, rest: &str) -> Result<(), PayloadError> {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
        let Some((inside, after)) = bracketed.split_once(']') else {
            return Err(PayloadError::invalid(
                field,
                link,
                "an IPv6 host needs a closing ']'",
            ));
        };
        if inside.is_empty()
            || !inside
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        {
            return Err(PayloadError::invalid(
                field,
                link,
                "the IPv6 host is malformed",
            ));
        }
        match after.strip_prefix(':') {
            Some(port) => (inside, Some(port)),
            None if after.is_empty() => (inside, None),
            None => {
                return Err(PayloadError::invalid(
                    field,
                    link,
                    "unexpected text after the IPv6 host",
                ));
            }
        }
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    if host.is_empty() {
        return Err(PayloadError::invalid(field, link, "the host is missing"));
    }
    if !host_port.starts_with('[') {
        let labels = host.strip_suffix('.').unwrap_or(host);
        if labels.split('.').any(str::is_empty) {
            return Err(PayloadError::invalid(
                field,
                link,
                "the host has an empty label",
            ));
        }
        let bad_char = |c: char| {
            c.is_ascii() && !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '%'))
        };
        if host.chars().any(bad_char) {
            return Err(PayloadError::invalid(
                field,
                link,
                "the host has a character hosts cannot hold",
            ));
        }
    }
    if let Some(port) = port {
        if port.is_empty()
            || !port.bytes().all(|b| b.is_ascii_digit())
            || port.parse::<u16>().is_err()
        {
            return Err(PayloadError::invalid(
                field,
                link,
                "the port must be a number from 0 to 65535",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_https_only_when_there_is_no_scheme() {
        for (input, expected) in [
            ("example.com", "https://example.com"),
            ("localhost:8080/x", "https://localhost:8080/x"),
            ("HTTP://Example.com/A", "http://Example.com/A"),
            ("https://a.b/p?q=1#f", "https://a.b/p?q=1#f"),
            (
                "https://user:pw@host.example:443/",
                "https://user:pw@host.example:443/",
            ),
            ("https://[::1]:8080/", "https://[::1]:8080/"),
            ("https://[2001:db8::1]", "https://[2001:db8::1]"),
            (
                "https://bücher.example/straße",
                "https://bücher.example/straße",
            ),
            ("https://example.com./", "https://example.com./"),
            ("custom+app://anything", "custom+app://anything"),
            ("x-app://", "x-app://"),
            ("localhost:8080", "https://localhost:8080"),
            ("example.com:443/a", "https://example.com:443/a"),
            ("urn:isbn:0451450523", "urn:isbn:0451450523"),
            ("Spotify:track:abc", "spotify:track:abc"),
            ("user@example.com/x", "https://user@example.com/x"),
        ] {
            let url = Url::new(input).unwrap();
            assert_eq!(url.as_str(), expected, "{input}");
            assert_eq!(url.to_string(), expected);
        }
    }

    #[test]
    fn rejects_malformed_links() {
        for (input, reason) in [
            ("https://a b", "whitespace"),
            ("https://a\u{7}", "control"),
            ("https://a/\u{202E}gpj.exe", "bidirectional"),
            ("https://a/<script>", "cannot hold raw"),
            ("1http://a", "scheme"),
            ("ht tp://a", "whitespace"),
            ("https://", "host is missing"),
            ("https:///path", "host is missing"),
            ("https://user@:80", "host is missing"),
            ("https://a..b", "empty label"),
            ("https://.a", "empty label"),
            ("https://a:", "port"),
            ("https://a:99999", "port"),
            ("https://a:8x", "port"),
            ("https://a$b", "hosts cannot hold"),
            ("https://[::1", "closing"),
            ("https://[]", "IPv6"),
            ("https://[zz]", "IPv6"),
            ("https://[::1]x", "after the IPv6"),
            ("https://[::1]:", "port"),
        ] {
            let err = Url::new(input).unwrap_err();
            assert!(err.to_string().contains(reason), "{input}: {err}");
        }
        assert_eq!(Url::new(""), Err(PayloadError::Empty { field: FIELD }));
    }

    #[test]
    fn a_leading_scheme_without_slashes_is_a_scheme() {
        for (input, subcommand) in [
            ("mailto:a@b.com", "qr email"),
            ("MAILTO:a@b.com", "qr email"),
            ("tel:+441", "qr tel"),
            ("sms:+441", "qr sms"),
            ("SMSTO:+441:hi", "qr sms"),
            ("geo:1,2", "qr geo"),
            ("tel:0123", "qr tel"),
            ("WIFI:T:WPA;S:x;;", "qr wifi"),
            ("mecard:N:x;;", "qr mecard"),
        ] {
            let err = Url::new(input).unwrap_err();
            assert!(
                err.to_string().contains("its own subcommand"),
                "{input}: {err}"
            );
            assert!(err.hint().unwrap().contains(subcommand), "{input}");
        }
        for input in [
            "javascript:alert(1)",
            "JavaScript://x%0aalert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,hi",
        ] {
            let err = Url::new(input).unwrap_err();
            assert!(err.to_string().contains("not allowed"), "{input}: {err}");
            assert!(err.hint().unwrap().contains("qr --"), "{input}");
        }
        let err = Url::new("https:example.com").unwrap_err();
        assert!(err.to_string().contains("needs //"), "{err}");
        assert!(err.hint().is_some());
    }

    #[test]
    fn hints_are_separate_from_reasons() {
        let err = Url::new("https://a b").unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid URL 'https://a b': contains whitespace"
        );
        assert_eq!(err.hint(), Some("write a space as %20"));
        let err = Url::with_field("website", "a<b").unwrap_err();
        assert!(
            err.to_string().starts_with("invalid website 'a<b'"),
            "{err}"
        );
        assert!(err.hint().unwrap().contains("percent-encode"));
    }
}
