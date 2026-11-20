//! Web links.

use std::fmt;

use super::PayloadError;

const FIELD: &str = "URL";

/// Characters RFC 3986 never allows raw in a URI. Non-ASCII text is
/// allowed (an IRI, RFC 3987), since phones open those as typed.
const FORBIDDEN: &[char] = &['"', '<', '>', '\\', '^', '`', '{', '|', '}'];

/// Schemes whose URIs must name a host.
const NEEDS_HOST: &[&str] = &["http", "https", "ftp", "ftps", "ws", "wss"];

/// A validated link, such as `https://example.com/a?b=c`.
///
/// Text without a `scheme://` prefix gets `https://`. The scheme is
/// lower-cased; the rest is kept exactly as given.
///
/// ```
/// use qr_core::payload::Url;
/// let url = Url::new("example.com/docs")?;
/// assert_eq!(url.to_string(), "https://example.com/docs");
/// assert!(Url::new("https://exa mple.com").is_err());
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
    /// character or a character URIs never allow raw (such as `"`, `<` or a backtick),
    /// has a malformed scheme, or lacks a valid host for a web scheme.
    pub fn new(link: &str) -> Result<Self, PayloadError> {
        if link.is_empty() {
            return Err(PayloadError::Empty { field: FIELD });
        }
        if let Some(character) = link.chars().find(|c| c.is_control()) {
            return Err(PayloadError::ControlCharacter {
                field: FIELD,
                character,
            });
        }
        if link.chars().any(char::is_whitespace) {
            return Err(PayloadError::invalid(
                FIELD,
                link,
                "contains whitespace; write a space as %20",
            ));
        }
        if link.contains(FORBIDDEN) {
            return Err(PayloadError::invalid(
                FIELD,
                link,
                "contains a character URLs cannot hold raw; percent-encode it",
            ));
        }
        let (scheme, rest) = match link.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            None => ("https".to_owned(), link),
        };
        if !is_scheme(&scheme) {
            return Err(PayloadError::invalid(
                FIELD,
                link,
                "the scheme must be a letter followed by letters, digits, '+', '-' or '.'",
            ));
        }
        if NEEDS_HOST.contains(&scheme.as_str()) {
            check_authority(link, rest)?;
        }
        Ok(Self(format!("{scheme}://{rest}")))
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

/// RFC 3986 section 3.1: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// Checks the authority (`[userinfo@]host[:port]`) at the start of `rest`.
fn check_authority(link: &str, rest: &str) -> Result<(), PayloadError> {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
        let Some((inside, after)) = bracketed.split_once(']') else {
            return Err(PayloadError::invalid(
                FIELD,
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
                FIELD,
                link,
                "the IPv6 host is malformed",
            ));
        }
        match after.strip_prefix(':') {
            Some(port) => (inside, Some(port)),
            None if after.is_empty() => (inside, None),
            None => {
                return Err(PayloadError::invalid(
                    FIELD,
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
        return Err(PayloadError::invalid(FIELD, link, "the host is missing"));
    }
    if !host_port.starts_with('[') {
        let labels = host.strip_suffix('.').unwrap_or(host);
        if labels.split('.').any(str::is_empty) {
            return Err(PayloadError::invalid(
                FIELD,
                link,
                "the host has an empty label",
            ));
        }
        let bad_char = |c: char| {
            c.is_ascii() && !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '%'))
        };
        if host.chars().any(bad_char) {
            return Err(PayloadError::invalid(
                FIELD,
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
                FIELD,
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
            ("https://a/<script>", "percent-encode"),
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
}
