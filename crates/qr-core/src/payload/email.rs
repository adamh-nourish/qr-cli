//! Email drafts as `mailto:` URIs (RFC 6068).

use std::fmt;

use super::PayloadError;
use super::escape::{self, Controls};

/// Characters left raw in an address: RFC 6068 `some-delims` that cannot
/// be mistaken for URI structure, plus `@`. `,` separates addresses and
/// `?`, `&`, `=`, `#`, `%` and `/` are URI syntax, so those are encoded.
const ADDRESS_KEEP: &[u8] = b"@!$'*+";

/// An email draft: `mailto:<to>?subject=..&cc=..&bcc=..&body=..`.
///
/// Every part is percent-encoded UTF-8, and line breaks in the body are
/// sent as `%0D%0A` as RFC 6068 requires.
///
/// ```
/// use qr_core::payload::Email;
/// let email = Email::new("ada@x.io")?.with_subject("Hi there")?;
/// assert_eq!(email.to_string(), "mailto:ada@x.io?subject=Hi%20there");
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Email {
    to: String,
    subject: Option<String>,
    body: Option<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
}

impl Email {
    /// A draft to `to`.
    ///
    /// # Errors
    ///
    /// [`PayloadError`] when `to` is not an address (see
    /// [`Email::with_cc`]).
    pub fn new(to: &str) -> Result<Self, PayloadError> {
        check_address(to)?;
        Ok(Self {
            to: to.to_owned(),
            subject: None,
            body: None,
            cc: Vec::new(),
            bcc: Vec::new(),
        })
    }

    /// Sets the subject line.
    ///
    /// # Errors
    ///
    /// [`PayloadError::ControlCharacter`] when it holds a control character
    /// such as a line break.
    pub fn with_subject(mut self, subject: &str) -> Result<Self, PayloadError> {
        escape::check_controls("subject", subject, Controls::Forbid)?;
        self.subject = Some(subject.to_owned());
        Ok(self)
    }

    /// Sets the message body.
    ///
    /// # Errors
    ///
    /// [`PayloadError::ControlCharacter`] when it holds a control character
    /// other than a tab or line break.
    pub fn with_body(mut self, body: &str) -> Result<Self, PayloadError> {
        escape::check_controls("body", body, Controls::AllowLineBreaks)?;
        self.body = Some(body.to_owned());
        Ok(self)
    }

    /// Adds a carbon-copy recipient.
    ///
    /// # Errors
    ///
    /// [`PayloadError`] when `address` is blank, has whitespace or a control
    /// character, or is not `local@domain`.
    pub fn with_cc(mut self, address: &str) -> Result<Self, PayloadError> {
        check_address(address)?;
        self.cc.push(address.to_owned());
        Ok(self)
    }

    /// Adds a blind carbon-copy recipient.
    ///
    /// # Errors
    ///
    /// As [`Email::with_cc`].
    pub fn with_bcc(mut self, address: &str) -> Result<Self, PayloadError> {
        check_address(address)?;
        self.bcc.push(address.to_owned());
        Ok(self)
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mailto:{}", escape::percent(&self.to, ADDRESS_KEEP))?;
        let list = |addresses: &[String]| {
            addresses
                .iter()
                .map(|a| escape::percent(a, ADDRESS_KEEP))
                .collect::<Vec<_>>()
                .join(",")
        };
        let fields = [
            (
                "subject",
                self.subject.as_deref().map(|s| escape::percent(s, b"")),
            ),
            ("cc", (!self.cc.is_empty()).then(|| list(&self.cc))),
            ("bcc", (!self.bcc.is_empty()).then(|| list(&self.bcc))),
            (
                "body",
                self.body
                    .as_deref()
                    .map(|b| escape::percent(&escape::crlf(b), b"")),
            ),
        ];
        let mut separator = '?';
        for (key, value) in fields {
            if let Some(value) = value {
                write!(f, "{separator}{key}={value}")?;
                separator = '&';
            }
        }
        Ok(())
    }
}

/// A light check that catches typos, not a full RFC 5322 parser: one
/// non-empty local part and domain around the last `@`, and no whitespace
/// or controls.
fn check_address(address: &str) -> Result<(), PayloadError> {
    const FIELD: &str = "email address";
    escape::check_required(FIELD, address, Controls::Forbid)?;
    if address.chars().any(char::is_whitespace) {
        return Err(PayloadError::invalid(FIELD, address, "contains whitespace"));
    }
    match address.rsplit_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() => Ok(()),
        _ => Err(PayloadError::invalid(
            FIELD,
            address,
            "expected the form name@example.com",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_every_part() {
        let email = Email::new("a+b@x.io")
            .and_then(|e| e.with_subject("Hi & bye?"))
            .and_then(|e| e.with_body("line 1\nline 2 100%"))
            .and_then(|e| e.with_cc("c@x.io"))
            .and_then(|e| e.with_cc("d,e@x.io"))
            .and_then(|e| e.with_bcc("f@x.io"))
            .unwrap();
        assert_eq!(
            email.to_string(),
            "mailto:a+b@x.io?subject=Hi%20%26%20bye%3F&cc=c@x.io,d%2Ce@x.io\
             &bcc=f@x.io&body=line%201%0D%0Aline%202%20100%25"
        );
        assert_eq!(Email::new("a@b").unwrap().to_string(), "mailto:a@b");
        let body_only = Email::new("a@b").and_then(|e| e.with_body("x")).unwrap();
        assert_eq!(body_only.to_string(), "mailto:a@b?body=x");
    }

    #[test]
    fn rejects_bad_addresses_and_fields() {
        for bad in ["", "nobody", "@x.io", "a@", "a b@x.io", "a\u{0}@x"] {
            assert!(Email::new(bad).is_err(), "{bad:?}");
        }
        let email = Email::new("a@b").unwrap();
        assert!(email.clone().with_subject("a\nb").is_err());
        assert!(email.clone().with_body("a\u{7}").is_err());
        assert!(email.clone().with_cc("x").is_err());
        assert!(email.with_bcc("").is_err());
    }
}
