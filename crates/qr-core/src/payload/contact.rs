//! Contact cards: vCard 3.0 (RFC 2426) and the shorter DoCoMo MeCard.

use std::fmt;

use super::escape::{self, Controls};
use super::{PayloadError, Url};

/// The details on a contact card. Only `name` is required.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Contact {
    /// The full name, as it should be displayed.
    pub name: String,
    /// The organisation (vCard only).
    pub org: Option<String>,
    /// The job title (vCard only).
    pub title: Option<String>,
    /// Phone numbers.
    pub phones: Vec<String>,
    /// Email addresses.
    pub emails: Vec<String>,
    /// A web site.
    pub url: Option<String>,
    /// A postal address, on one line or several.
    pub address: Option<String>,
    /// A free-form note.
    pub note: Option<String>,
}

impl Contact {
    /// A contact with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Checks every field: the name must not be blank, phone numbers and
    /// emails must not be blank, an email needs an `@`, and only tab and
    /// line breaks are allowed among control characters.
    ///
    /// The URL is checked and normalised as for `qr url` (`https://` is
    /// added when it has no scheme), so the validated contact is returned.
    fn validate(mut self) -> Result<Self, PayloadError> {
        escape::check_required("name", &self.name, Controls::AllowLineBreaks)?;
        for phone in &self.phones {
            escape::check_required("phone number", phone, Controls::Forbid)?;
        }
        for email in &self.emails {
            escape::check_required("email address", email, Controls::Forbid)?;
            if !email.contains('@') {
                return Err(PayloadError::invalid(
                    "email address",
                    email,
                    "an email address needs an '@'",
                ));
            }
        }
        if let Some(url) = &self.url {
            // A URI value: it is written unescaped, so it must be a valid
            // link (which also rules out line breaks).
            self.url = Some(Url::with_field("URL", url)?.as_str().to_owned());
        }
        for (field, value) in [
            ("organisation", &self.org),
            ("title", &self.title),
            ("address", &self.address),
            ("note", &self.note),
        ] {
            if let Some(value) = value {
                escape::check_controls(field, value, Controls::AllowLineBreaks)?;
            }
        }
        Ok(self)
    }
}

/// A vCard 3.0 contact card (RFC 2426).
///
/// Text values escape `\ ; ,` and line breaks (the URL, a URI value, is
/// written as given); lines end in CRLF and are not folded. The structured
/// `N` property is derived from the name: the last word is the family name
/// and the rest the given names.
///
/// ```
/// use qr_core::payload::{Contact, VCard};
/// let card = VCard::new(Contact {
///     phones: vec!["+441234".into()],
///     ..Contact::new("Ada Lovelace")
/// })?;
/// assert_eq!(
///     card.to_string(),
///     "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Lovelace;Ada;;;\r\nFN:Ada Lovelace\r\n\
///      TEL:+441234\r\nEND:VCARD\r\n"
/// );
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VCard(Contact);

impl VCard {
    /// Validates the contact.
    ///
    /// # Errors
    ///
    /// [`PayloadError`] when the name is blank, a phone number or email is
    /// blank, an email has no `@`, the URL is not a valid link (see
    /// [`Url::new`]), or a field holds a control character other than a
    /// tab or line break.
    pub fn new(contact: Contact) -> Result<Self, PayloadError> {
        Ok(Self(contact.validate()?))
    }
}

impl fmt::Display for VCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = &self.0;
        let name = c.name.trim();
        let (given, family) = match name.rsplit_once(char::is_whitespace) {
            Some((given, family)) => (given.trim_end(), family),
            None => ("", name),
        };
        f.write_str("BEGIN:VCARD\r\nVERSION:3.0\r\n")?;
        write!(
            f,
            "N:{};{};;;\r\nFN:{}\r\n",
            escape::text(family),
            escape::text(given),
            escape::text(name)
        )?;
        let line = |f: &mut fmt::Formatter<'_>, key: &str, value: &Option<String>| {
            value
                .as_ref()
                .map_or(Ok(()), |v| write!(f, "{key}:{}\r\n", escape::text(v)))
        };
        line(f, "ORG", &c.org)?;
        line(f, "TITLE", &c.title)?;
        for phone in &c.phones {
            write!(f, "TEL:{}\r\n", escape::text(phone))?;
        }
        for email in &c.emails {
            write!(f, "EMAIL:{}\r\n", escape::text(email))?;
        }
        if let Some(url) = &c.url {
            // URL is a URI value (RFC 2426 section 3.6.8), not TEXT: a comma
            // in it is data, so it is not escaped.
            write!(f, "URL:{url}\r\n")?;
        }
        if let Some(address) = &c.address {
            // ADR is structured; the whole address goes in the street part.
            write!(f, "ADR:;;{};;;;\r\n", escape::text(address))?;
        }
        line(f, "NOTE", &c.note)?;
        f.write_str("END:VCARD\r\n")
    }
}

/// A DoCoMo MeCard: `MECARD:N:<name>;TEL:<phone>;...;;`.
///
/// Values escape `\ ; , : "` with a backslash. MeCard has no organisation
/// or job title, so a contact with either is refused rather than silently
/// trimmed.
///
/// ```
/// use qr_core::payload::{Contact, MeCard};
/// let card = MeCard::new(Contact {
///     phones: vec!["+441234".into()],
///     ..Contact::new("Ada")
/// })?;
/// assert_eq!(card.to_string(), "MECARD:N:Ada;TEL:+441234;;");
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MeCard(Contact);

impl MeCard {
    /// Validates the contact.
    ///
    /// # Errors
    ///
    /// As [`VCard::new`], and [`PayloadError::Inconsistent`] when the
    /// contact has an organisation or a title.
    pub fn new(contact: Contact) -> Result<Self, PayloadError> {
        let contact = contact.validate()?;
        if contact.org.is_some() || contact.title.is_some() {
            return Err(PayloadError::inconsistent(
                "a mecard has no organisation or title",
                Some("use qr vcard for --org and --title"),
            ));
        }
        Ok(Self(contact))
    }
}

impl fmt::Display for MeCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = &self.0;
        write!(f, "MECARD:N:{};", escape::backslash(c.name.trim()))?;
        for phone in &c.phones {
            write!(f, "TEL:{};", escape::backslash(phone))?;
        }
        for email in &c.emails {
            write!(f, "EMAIL:{};", escape::backslash(email))?;
        }
        for (key, value) in [("URL", &c.url), ("ADR", &c.address), ("NOTE", &c.note)] {
            if let Some(value) = value {
                write!(f, "{key}:{};", escape::backslash(value))?;
            }
        }
        f.write_str(";")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Contact {
        Contact {
            org: Some("ACME; Inc.".into()),
            title: Some("Engineer, Lead".into()),
            phones: vec!["+44 1234".into(), "555".into()],
            emails: vec!["ada@x.io".into()],
            url: Some("https://x.io/a,b".into()),
            address: Some("1 Road\nTown".into()),
            note: Some(r"back\slash".into()),
            ..Contact::new("  Augusta Ada  King  ")
        }
    }

    #[test]
    fn vcard_writes_every_field_escaped() {
        let card = VCard::new(full()).unwrap();
        assert_eq!(
            card.to_string(),
            "BEGIN:VCARD\r\nVERSION:3.0\r\nN:King;Augusta Ada;;;\r\nFN:Augusta Ada  King\r\n\
             ORG:ACME\\; Inc.\r\nTITLE:Engineer\\, Lead\r\nTEL:+44 1234\r\nTEL:555\r\n\
             EMAIL:ada@x.io\r\nURL:https://x.io/a,b\r\nADR:;;1 Road\\nTown;;;;\r\n\
             NOTE:back\\\\slash\r\nEND:VCARD\r\n"
        );
        let single = VCard::new(Contact::new("Cher")).unwrap();
        assert!(single.to_string().contains("\r\nN:Cher;;;;\r\nFN:Cher\r\n"));
    }

    #[test]
    fn mecard_writes_every_field_escaped() {
        let card = MeCard::new(Contact {
            org: None,
            title: None,
            ..full()
        })
        .unwrap();
        assert_eq!(
            card.to_string(),
            "MECARD:N:Augusta Ada  King;TEL:+44 1234;TEL:555;EMAIL:ada@x.io;\
             URL:https\\://x.io/a\\,b;ADR:1 Road\nTown;NOTE:back\\\\slash;;"
        );
    }

    #[test]
    fn mecard_refuses_vcard_only_fields() {
        assert!(matches!(
            MeCard::new(full()),
            Err(PayloadError::Inconsistent { .. })
        ));
    }

    #[test]
    fn validation() {
        assert_eq!(
            VCard::new(Contact::new(" ")),
            Err(PayloadError::Empty { field: "name" })
        );
        let bad_email = Contact {
            emails: vec!["nobody".into()],
            ..Contact::new("A")
        };
        assert!(matches!(
            VCard::new(bad_email),
            Err(PayloadError::Invalid {
                field: "email address",
                ..
            })
        ));
        let blank_phone = Contact {
            phones: vec![String::new()],
            ..Contact::new("A")
        };
        assert!(MeCard::new(blank_phone).is_err());
        let control_note = Contact {
            note: Some("\u{1b}[31m".into()),
            ..Contact::new("A")
        };
        assert!(matches!(
            VCard::new(control_note),
            Err(PayloadError::ControlCharacter { field: "note", .. })
        ));
        let broken_url = Contact {
            url: Some("https://x\n.io".into()),
            ..Contact::new("A")
        };
        assert!(matches!(
            VCard::new(broken_url),
            Err(PayloadError::ControlCharacter { field: "URL", .. })
        ));
        let blank_email = Contact {
            emails: vec![" ".into()],
            ..Contact::new("A")
        };
        assert!(VCard::new(blank_email).is_err());
    }

    #[test]
    fn urls_are_normalised_like_qr_url() {
        let with_url = |url: &str| Contact {
            url: Some(url.into()),
            ..Contact::new("A")
        };
        let card = VCard::new(with_url("example.com/a")).unwrap().to_string();
        assert!(card.contains("URL:https://example.com/a\r\n"), "{card}");
        let card = MeCard::new(with_url("example.com")).unwrap().to_string();
        assert!(card.contains("URL:https\\://example.com;"), "{card}");
        assert!(matches!(
            VCard::new(with_url("javascript:alert(1)")),
            Err(PayloadError::Invalid { field: "URL", .. })
        ));
        assert!(VCard::new(with_url("https://exa mple.com")).is_err());
    }
}
