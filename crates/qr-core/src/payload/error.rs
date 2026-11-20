//! Errors returned by the payload builders.

use thiserror::Error;

/// Why a structured payload could not be built.
///
/// Messages are lowercase and quote the offending value, except for secrets
/// (a WiFi password is never echoed back).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum PayloadError {
    /// A required field is empty.
    #[error("the {field} is empty")]
    Empty {
        /// The field, such as `SSID`.
        field: &'static str,
    },

    /// A field holds a control character the format cannot carry.
    #[error("the {field} contains the control character U+{:04X}", u32::from(*.character))]
    ControlCharacter {
        /// The field.
        field: &'static str,
        /// The offending character.
        character: char,
    },

    /// A field holds a bidirectional formatting character (such as U+202E,
    /// right-to-left override), which can make a phone show the text in a
    /// misleading order.
    #[error(
        "the {field} contains the bidirectional formatting character U+{:04X}",
        u32::from(*.character)
    )]
    BidiCharacter {
        /// The field.
        field: &'static str,
        /// The offending character.
        character: char,
    },

    /// A field is longer than the format allows.
    #[error("the {field} is {len} bytes long; the most is {max}")]
    TooLong {
        /// The field.
        field: &'static str,
        /// Its length in bytes.
        len: usize,
        /// The most bytes allowed.
        max: usize,
    },

    /// A field's value is malformed or out of range.
    #[error("invalid {field} '{value}': {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// The value as given.
        value: String,
        /// What is wrong with it, as a lowercase phrase.
        reason: &'static str,
        /// What to do about it, if there is one obvious fix.
        hint: Option<&'static str>,
    },

    /// A secret is malformed. Unlike [`Self::Invalid`], the value is not
    /// kept, so it cannot leak into a message or log.
    #[error("invalid {field}: {reason}")]
    InvalidSecret {
        /// The field.
        field: &'static str,
        /// What is wrong with it. It may give the secret's length, never
        /// its content.
        reason: String,
        /// What to do about it, if there is one obvious fix.
        hint: Option<&'static str>,
    },

    /// The fields do not make sense together.
    #[error("{message}")]
    Inconsistent {
        /// What is wrong.
        message: &'static str,
        /// What to do about it, if there is one obvious fix.
        hint: Option<&'static str>,
    },
}

impl PayloadError {
    pub(crate) fn invalid(field: &'static str, value: &str, reason: &'static str) -> Self {
        Self::Invalid {
            field,
            value: value.to_owned(),
            reason,
            hint: None,
        }
    }

    pub(crate) fn invalid_with_hint(
        field: &'static str,
        value: &str,
        reason: &'static str,
        hint: &'static str,
    ) -> Self {
        Self::Invalid {
            field,
            value: value.to_owned(),
            reason,
            hint: Some(hint),
        }
    }

    pub(crate) const fn inconsistent(message: &'static str, hint: Option<&'static str>) -> Self {
        Self::Inconsistent { message, hint }
    }

    /// One suggestion for fixing the input, kept apart from the message so
    /// that callers can show it as a hint.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::ControlCharacter { .. } => Some("remove the control character from the value"),
            Self::BidiCharacter { .. } => Some(
                "remove it: it can make the text display in a different order from what it holds",
            ),
            Self::Invalid { hint, .. }
            | Self::InvalidSecret { hint, .. }
            | Self::Inconsistent { hint, .. } => *hint,
            Self::Empty { .. } | Self::TooLong { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_quote_values_but_not_secrets() {
        assert_eq!(
            PayloadError::Empty { field: "SSID" }.to_string(),
            "the SSID is empty"
        );
        assert_eq!(
            PayloadError::ControlCharacter {
                field: "name",
                character: '\u{1b}'
            }
            .to_string(),
            "the name contains the control character U+001B"
        );
        assert_eq!(
            PayloadError::TooLong {
                field: "SSID",
                len: 40,
                max: 32
            }
            .to_string(),
            "the SSID is 40 bytes long; the most is 32"
        );
        assert_eq!(
            PayloadError::invalid("latitude", "91", "must be between -90 and 90").to_string(),
            "invalid latitude '91': must be between -90 and 90"
        );
        assert_eq!(
            PayloadError::InvalidSecret {
                field: "password",
                reason: "too short".to_owned(),
                hint: None,
            }
            .to_string(),
            "invalid password: too short"
        );
        assert_eq!(
            PayloadError::inconsistent("the event ends before it starts", Some("swap them"))
                .to_string(),
            "the event ends before it starts"
        );
    }

    #[test]
    fn hints_are_kept_apart_from_messages() {
        let err = PayloadError::invalid_with_hint("URL", "a b", "contains whitespace", "use %20");
        assert_eq!(err.to_string(), "invalid URL 'a b': contains whitespace");
        assert_eq!(err.hint(), Some("use %20"));
        assert_eq!(PayloadError::invalid("URL", "x", "bad").hint(), None);
        assert_eq!(PayloadError::Empty { field: "SSID" }.hint(), None);
        assert!(
            PayloadError::ControlCharacter {
                field: "SSID",
                character: '\n'
            }
            .hint()
            .is_some()
        );
    }
}
