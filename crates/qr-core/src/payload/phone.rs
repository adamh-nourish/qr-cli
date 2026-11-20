//! Phone numbers, `tel:` links (RFC 3966) and `SMSTO:` text messages.

use std::fmt;

use super::PayloadError;
use super::escape::{self, Controls};

/// The most digits a number may have: E.164 allows 15, and extensions or
/// dialling prefixes need a little more.
pub const MAX_PHONE_DIGITS: usize = 24;

/// The longest message an `SMSTO:` payload carries, in characters. Longer
/// texts are split by the phone anyway, and the QR code would be large.
const MAX_SMS_CHARS: usize = 1_000;

/// A phone number reduced to an optional leading `+` and digits.
///
/// Spaces and the RFC 3966 visual separators `- . ( )` are accepted and
/// dropped, so `+44 (0)20 7946-0000` becomes `+4402079460000`. Keeping
/// only digits also lets the QR encoder use its compact numeric mode.
///
/// ```
/// use qr_core::payload::PhoneNumber;
/// assert_eq!(PhoneNumber::new("+44 20 7946-0000")?.as_str(), "+442079460000");
/// assert!(PhoneNumber::new("call me").is_err());
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhoneNumber(String);

impl PhoneNumber {
    /// Validates and normalises `number`.
    ///
    /// # Errors
    ///
    /// [`PayloadError`] when the number is blank, holds anything but
    /// digits, a leading `+` and separators, or has more than
    /// [`MAX_PHONE_DIGITS`] digits.
    pub fn new(number: &str) -> Result<Self, PayloadError> {
        const FIELD: &str = "phone number";
        escape::check_required(FIELD, number, Controls::Forbid)?;
        let trimmed = number.trim();
        let (plus, rest) = match trimmed.strip_prefix('+') {
            Some(rest) => ("+", rest),
            None => ("", trimmed),
        };
        let mut digits = String::with_capacity(rest.len() + 1);
        digits.push_str(plus);
        for c in rest.chars() {
            match c {
                '0'..='9' => digits.push(c),
                ' ' | '-' | '.' | '(' | ')' => {}
                _ => {
                    return Err(PayloadError::invalid(
                        FIELD,
                        number,
                        "use digits, a leading '+', spaces and - . ( ) only",
                    ));
                }
            }
        }
        let count = digits.len() - plus.len();
        if count == 0 {
            return Err(PayloadError::invalid(FIELD, number, "it has no digits"));
        }
        if count > MAX_PHONE_DIGITS {
            return Err(PayloadError::TooLong {
                field: FIELD,
                len: count,
                max: MAX_PHONE_DIGITS,
            });
        }
        Ok(Self(digits))
    }

    /// The normalised number.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PhoneNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A number to call: `tel:+441234567890`.
///
/// ```
/// use qr_core::payload::Tel;
/// assert_eq!(Tel::new("+44 1234 567890")?.to_string(), "tel:+441234567890");
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tel(PhoneNumber);

impl Tel {
    /// Validates the number (see [`PhoneNumber::new`]).
    ///
    /// # Errors
    ///
    /// As [`PhoneNumber::new`].
    pub fn new(number: &str) -> Result<Self, PayloadError> {
        PhoneNumber::new(number).map(Self)
    }
}

impl fmt::Display for Tel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tel:{}", self.0)
    }
}

/// A text message draft: `SMSTO:<number>:<message>`, the form ZXing,
/// Android and iOS cameras read.
///
/// The message is the rest of the payload, so it needs no escaping and
/// may hold `:` and line breaks.
///
/// ```
/// use qr_core::payload::Sms;
/// let sms = Sms::new("+44 7700 900123", Some("Running late: 10 min"))?;
/// assert_eq!(sms.to_string(), "SMSTO:+447700900123:Running late: 10 min");
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sms {
    number: PhoneNumber,
    message: Option<String>,
}

impl Sms {
    /// Validates the number and message.
    ///
    /// # Errors
    ///
    /// As [`PhoneNumber::new`], and [`PayloadError`] when the message holds
    /// a control character other than a tab or line break, or is longer
    /// than 1,000 characters.
    pub fn new(number: &str, message: Option<&str>) -> Result<Self, PayloadError> {
        let number = PhoneNumber::new(number)?;
        if let Some(message) = message {
            escape::check_controls("message", message, Controls::AllowLineBreaks)?;
            let len = message.chars().count();
            if len > MAX_SMS_CHARS {
                return Err(PayloadError::TooLong {
                    field: "message",
                    len,
                    max: MAX_SMS_CHARS,
                });
            }
        }
        Ok(Self {
            number,
            message: message.filter(|m| !m.is_empty()).map(str::to_owned),
        })
    }
}

impl fmt::Display for Sms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SMSTO:{}:", self.number)?;
        if let Some(message) = &self.message {
            f.write_str(message)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_normalised() {
        for (input, expected) in [
            ("+44 (0)20 7946-0000", "+4402079460000".to_owned()),
            ("  555.1234 ", "5551234".to_owned()),
            ("+1", "+1".to_owned()),
        ] {
            assert_eq!(PhoneNumber::new(input).unwrap().as_str(), expected);
        }
        assert_eq!(PhoneNumber::new("12").unwrap().to_string(), "12");
    }

    #[test]
    fn bad_numbers_are_refused() {
        assert_eq!(
            PhoneNumber::new(" "),
            Err(PayloadError::Empty {
                field: "phone number"
            })
        );
        for (input, reason) in [
            ("12a", "digits"),
            ("1+2", "digits"),
            ("++1", "digits"),
            ("+", "no digits"),
            ("()-", "no digits"),
        ] {
            let err = PhoneNumber::new(input).unwrap_err();
            assert!(err.to_string().contains(reason), "{input}: {err}");
        }
        assert!(matches!(
            PhoneNumber::new(&"1".repeat(25)),
            Err(PayloadError::TooLong { len: 25, .. })
        ));
        assert!(PhoneNumber::new(&"1".repeat(24)).is_ok());
        assert!(matches!(
            PhoneNumber::new("1\t2"),
            Err(PayloadError::ControlCharacter { .. })
        ));
    }

    #[test]
    fn tel_and_sms_payloads() {
        assert_eq!(
            Tel::new("+1 555 0100").unwrap().to_string(),
            "tel:+15550100"
        );
        assert!(Tel::new("x").is_err());
        assert_eq!(Sms::new("123", None).unwrap().to_string(), "SMSTO:123:");
        assert_eq!(Sms::new("123", Some("")).unwrap().to_string(), "SMSTO:123:");
        assert_eq!(
            Sms::new("123", Some("a:b\nc")).unwrap().to_string(),
            "SMSTO:123:a:b\nc"
        );
        assert!(matches!(
            Sms::new("123", Some("\u{1b}")),
            Err(PayloadError::ControlCharacter { .. })
        ));
        assert!(matches!(
            Sms::new("123", Some(&"é".repeat(1_001))),
            Err(PayloadError::TooLong { len: 1_001, .. })
        ));
        assert!(Sms::new("123", Some(&"é".repeat(1_000))).is_ok());
    }
}
