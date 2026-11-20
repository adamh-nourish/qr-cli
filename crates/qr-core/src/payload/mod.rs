//! Structured payloads: the text a phone camera turns into an action.
//!
//! Each builder validates its fields when it is created and writes the
//! payload through [`Display`](std::fmt::Display), so a built value is
//! always well formed:
//!
//! | Type | Format |
//! |---|---|
//! | [`Url`] | a web link, `https://` added when there is no scheme |
//! | [`Wifi`] | ZXing `WIFI:T:..;S:..;P:..;;` |
//! | [`VCard`] | vCard 3.0 (RFC 2426) |
//! | [`MeCard`] | DoCoMo `MECARD:N:..;;` |
//! | [`Email`] | `mailto:` URI (RFC 6068) |
//! | [`Sms`] | `SMSTO:<number>:<message>` |
//! | [`Tel`] | `tel:` URI (RFC 3966) |
//! | [`Geo`] | `geo:` URI (RFC 5870) |
//! | [`Event`] | iCalendar `VEVENT` (RFC 5545) |
//!
//! [`Payload`] wraps any of them, for code that handles every kind (the CLI
//! subcommands, and later batch input).
//!
//! ```
//! use qr_core::payload::{Geo, Payload};
//! let payload = Payload::from(Geo::new(51.5, -0.12)?);
//! assert_eq!(payload.kind(), "geo");
//! assert_eq!(payload.to_string(), "geo:51.5,-0.12");
//! # Ok::<(), qr_core::payload::PayloadError>(())
//! ```

use std::fmt;

mod contact;
mod datetime;
mod email;
mod error;
mod escape;
mod event;
mod geo;
mod phone;
mod url;
mod wifi;

pub use self::contact::{Contact, MeCard, VCard};
pub use self::datetime::{Date, DateTime, DateTimeError, Part, Time};
pub use self::email::Email;
pub use self::error::PayloadError;
pub use self::event::Event;
pub use self::geo::Geo;
pub use self::phone::{MAX_PHONE_DIGITS, PhoneNumber, Sms, Tel};
pub use self::url::Url;
pub use self::wifi::{MAX_SSID_BYTES, UnknownWifiAuth, Wifi, WifiAuth};

/// Any structured payload.
///
/// Its [`Debug`](fmt::Debug) output never contains a WiFi password.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Payload {
    /// A web link.
    Url(Url),
    /// WiFi network credentials.
    Wifi(Wifi),
    /// A vCard 3.0 contact.
    VCard(VCard),
    /// A MeCard contact.
    MeCard(MeCard),
    /// An email draft.
    Email(Email),
    /// A text message draft.
    Sms(Sms),
    /// A phone number to call.
    Tel(Tel),
    /// A location.
    Geo(Geo),
    /// A calendar event.
    Event(Event),
}

impl Payload {
    /// The lowercase name of the kind, which is also the CLI subcommand.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Url(_) => "url",
            Self::Wifi(_) => "wifi",
            Self::VCard(_) => "vcard",
            Self::MeCard(_) => "mecard",
            Self::Email(_) => "email",
            Self::Sms(_) => "sms",
            Self::Tel(_) => "tel",
            Self::Geo(_) => "geo",
            Self::Event(_) => "event",
        }
    }
}

impl fmt::Display for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(p) => p.fmt(f),
            Self::Wifi(p) => p.fmt(f),
            Self::VCard(p) => p.fmt(f),
            Self::MeCard(p) => p.fmt(f),
            Self::Email(p) => p.fmt(f),
            Self::Sms(p) => p.fmt(f),
            Self::Tel(p) => p.fmt(f),
            Self::Geo(p) => p.fmt(f),
            Self::Event(p) => p.fmt(f),
        }
    }
}

macro_rules! from_builder {
    ($($variant:ident),*) => {$(
        impl From<$variant> for Payload {
            fn from(payload: $variant) -> Self {
                Self::$variant(payload)
            }
        }
    )*};
}

from_builder!(Url, Wifi, VCard, MeCard, Email, Sms, Tel, Geo, Event);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_name_and_displays_its_builder() {
        let wifi = Wifi::new("Home", Some("hunter2hunter2"), WifiAuth::Wpa, false).unwrap();
        let cases: Vec<(Payload, &str)> = vec![
            (Url::new("example.com").unwrap().into(), "url"),
            (wifi.into(), "wifi"),
            (VCard::new(Contact::new("A")).unwrap().into(), "vcard"),
            (MeCard::new(Contact::new("A")).unwrap().into(), "mecard"),
            (Email::new("a@b").unwrap().into(), "email"),
            (Sms::new("123", None).unwrap().into(), "sms"),
            (Tel::new("123").unwrap().into(), "tel"),
            (Geo::new(0.0, 0.0).unwrap().into(), "geo"),
            (Event::new("x", "2026-01-01", None).unwrap().into(), "event"),
        ];
        for (payload, kind) in cases {
            assert_eq!(payload.kind(), kind);
            let text = payload.to_string();
            let inner = match &payload {
                Payload::Url(p) => p.to_string(),
                Payload::Wifi(p) => p.to_string(),
                Payload::VCard(p) => p.to_string(),
                Payload::MeCard(p) => p.to_string(),
                Payload::Email(p) => p.to_string(),
                Payload::Sms(p) => p.to_string(),
                Payload::Tel(p) => p.to_string(),
                Payload::Geo(p) => p.to_string(),
                Payload::Event(p) => p.to_string(),
            };
            assert_eq!(text, inner);
            assert!(!format!("{payload:?}").contains("hunter2"));
        }
    }
}
