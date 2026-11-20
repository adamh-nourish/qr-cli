//! WiFi network credentials (the ZXing `WIFI:` format that phone cameras
//! read).

use std::fmt;
use std::str::FromStr;

use super::PayloadError;
use super::escape::{self, Controls};

/// The longest SSID IEEE 802.11 allows, in bytes.
pub const MAX_SSID_BYTES: usize = 32;

/// How a WiFi network authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WifiAuth {
    /// WPA, WPA2 or WPA3 personal (`T:WPA`).
    #[default]
    Wpa,
    /// Legacy WEP (`T:WEP`).
    Wep,
    /// An open network (`T:nopass`).
    NoPass,
}

impl WifiAuth {
    /// The value of the `T:` field.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Wpa => "WPA",
            Self::Wep => "WEP",
            Self::NoPass => "nopass",
        }
    }
}

impl fmt::Display for WifiAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Wpa => "wpa",
            Self::Wep => "wep",
            Self::NoPass => "nopass",
        })
    }
}

/// The error from parsing a [`WifiAuth`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown authentication '{0}': expected wpa, wep or nopass")]
pub struct UnknownWifiAuth(String);

impl FromStr for WifiAuth {
    type Err = UnknownWifiAuth;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "wpa" | "wpa2" | "wpa3" => Ok(Self::Wpa),
            "wep" => Ok(Self::Wep),
            "nopass" | "none" | "open" => Ok(Self::NoPass),
            _ => Err(UnknownWifiAuth(s.to_owned())),
        }
    }
}

/// A network to join, encoded as
/// `WIFI:T:<auth>;S:<ssid>;P:<password>;H:true;;`.
///
/// The SSID and password escape `\ ; , : "` with a backslash. The password
/// is checked against the authentication type: WPA needs 8 to 63
/// characters or 64 hex digits, WEP needs 5 or 13 characters or 10 or 26
/// hex digits, and an open network takes none.
///
/// ```
/// use qr_core::payload::{Wifi, WifiAuth};
/// let wifi = Wifi::new("Home;Net", Some("s3cr;t-pass"), WifiAuth::Wpa, false)?;
/// assert_eq!(wifi.to_string(), r"WIFI:T:WPA;S:Home\;Net;P:s3cr\;t-pass;;");
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Wifi {
    ssid: String,
    password: Option<String>,
    auth: WifiAuth,
    hidden: bool,
}

impl Wifi {
    /// Validates the network details.
    ///
    /// # Errors
    ///
    /// [`PayloadError`] when the SSID is empty, longer than
    /// [`MAX_SSID_BYTES`] or holds a control character, or when the
    /// password does not suit `auth` (see the type documentation).
    pub fn new(
        ssid: &str,
        password: Option<&str>,
        auth: WifiAuth,
        hidden: bool,
    ) -> Result<Self, PayloadError> {
        if ssid.is_empty() {
            return Err(PayloadError::Empty { field: "SSID" });
        }
        escape::check_controls("SSID", ssid, Controls::Forbid)?;
        if ssid.len() > MAX_SSID_BYTES {
            return Err(PayloadError::TooLong {
                field: "SSID",
                len: ssid.len(),
                max: MAX_SSID_BYTES,
            });
        }
        if let Some(password) = password {
            check_password(password, auth)?;
        } else if auth != WifiAuth::NoPass {
            return Err(PayloadError::inconsistent(
                "a wpa or wep network needs a password",
                Some("pass --password-stdin (or --password), or --auth nopass for an open network"),
            ));
        }
        Ok(Self {
            ssid: ssid.to_owned(),
            password: password.map(str::to_owned),
            auth,
            hidden,
        })
    }

    /// The network name.
    pub fn ssid(&self) -> &str {
        &self.ssid
    }

    /// The authentication type.
    pub const fn auth(&self) -> WifiAuth {
        self.auth
    }
}

/// The password is left out so that it cannot end up in a log.
impl fmt::Debug for Wifi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wifi")
            .field("ssid", &self.ssid)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("auth", &self.auth)
            .field("hidden", &self.hidden)
            .finish()
    }
}

impl fmt::Display for Wifi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WIFI:T:{};S:{};",
            self.auth.code(),
            escape::backslash(&self.ssid)
        )?;
        if let Some(password) = &self.password {
            write!(f, "P:{};", escape::backslash(password))?;
        }
        if self.hidden {
            f.write_str("H:true;")?;
        }
        f.write_str(";")
    }
}

fn check_password(password: &str, auth: WifiAuth) -> Result<(), PayloadError> {
    const FIELD: &str = "password";
    if auth == WifiAuth::NoPass {
        return Err(PayloadError::inconsistent(
            "an open (nopass) network takes no password",
            Some("drop the password, or pass --auth wpa or --auth wep"),
        ));
    }
    if let Some(character) = password.chars().find(|c| c.is_control()) {
        // The character is not secret enough to hide, but the value is.
        return Err(PayloadError::ControlCharacter {
            field: FIELD,
            character,
        });
    }
    let len = password.chars().count();
    let hex = password.bytes().all(|b| b.is_ascii_hexdigit());
    let ok = match auth {
        WifiAuth::Wpa => (8..=63).contains(&len) || (len == 64 && hex),
        WifiAuth::Wep => matches!(len, 5 | 13) || (matches!(len, 10 | 26) && hex),
        WifiAuth::NoPass => false,
    };
    if ok {
        Ok(())
    } else {
        // The length is not secret enough to hide, but the value is.
        let rule = if auth == WifiAuth::Wpa {
            "a wpa password has 8 to 63 characters, or 64 hex digits"
        } else {
            "a wep key has 5 or 13 characters, or 10 or 26 hex digits"
        };
        let plural = if len == 1 { "" } else { "s" };
        Err(PayloadError::InvalidSecret {
            field: FIELD,
            reason: format!("{rule} (got {len} character{plural})"),
            hint: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_every_field() {
        let wifi = Wifi::new(r#"My "Net""#, Some(r"a\b:c,d;e"), WifiAuth::Wpa, true).unwrap();
        assert_eq!(
            wifi.to_string(),
            r#"WIFI:T:WPA;S:My \"Net\";P:a\\b\:c\,d\;e;H:true;;"#
        );
        assert_eq!(wifi.ssid(), r#"My "Net""#);
        assert_eq!(wifi.auth(), WifiAuth::Wpa);
        let open = Wifi::new("Cafe", None, WifiAuth::NoPass, false).unwrap();
        assert_eq!(open.to_string(), "WIFI:T:nopass;S:Cafe;;");
        let wep = Wifi::new("Old", Some("abcde"), WifiAuth::Wep, false).unwrap();
        assert_eq!(wep.to_string(), "WIFI:T:WEP;S:Old;P:abcde;;");
    }

    #[test]
    fn debug_redacts_the_password() {
        let wifi = Wifi::new("Home", Some("hunter2hunter2"), WifiAuth::Wpa, false).unwrap();
        let debug = format!("{wifi:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn password_lengths_follow_the_auth_type() {
        let hex64 = "a".repeat(64);
        let ok = [
            (WifiAuth::Wpa, "12345678"),
            (WifiAuth::Wpa, &"x".repeat(63)),
            (WifiAuth::Wpa, &hex64),
            (WifiAuth::Wpa, "ünïcödé!"),
            (WifiAuth::Wep, "12345"),
            (WifiAuth::Wep, "1234567890abc"),
            (WifiAuth::Wep, "0123456789"),
            (WifiAuth::Wep, "0123456789abcdef0123456789"),
        ];
        for (auth, password) in ok {
            assert!(
                Wifi::new("n", Some(password), auth, false).is_ok(),
                "{auth} {password}"
            );
        }
        let bad = [
            (WifiAuth::Wpa, "1234567"),
            (WifiAuth::Wpa, &"x".repeat(64)),
            (WifiAuth::Wpa, &"x".repeat(65)),
            (WifiAuth::Wep, "1234"),
            (WifiAuth::Wep, "zzzzzzzzzz"),
        ];
        for (auth, password) in bad {
            let err = Wifi::new("n", Some(password), auth, false).unwrap_err();
            assert!(matches!(err, PayloadError::InvalidSecret { .. }), "{err}");
            assert!(!err.to_string().contains(password));
            let len = password.chars().count();
            assert!(
                err.to_string().contains(&format!("(got {len} characters)")),
                "{err}"
            );
        }
    }

    #[test]
    fn rejects_inconsistent_or_malformed_fields() {
        assert_eq!(
            Wifi::new("", None, WifiAuth::NoPass, false),
            Err(PayloadError::Empty { field: "SSID" })
        );
        assert!(matches!(
            Wifi::new(&"s".repeat(33), None, WifiAuth::NoPass, false),
            Err(PayloadError::TooLong {
                len: 33,
                max: 32,
                ..
            })
        ));
        assert!(matches!(
            Wifi::new("a\nb", None, WifiAuth::NoPass, false),
            Err(PayloadError::ControlCharacter { .. })
        ));
        assert!(matches!(
            Wifi::new("a", Some("pass\nword"), WifiAuth::Wpa, false),
            Err(PayloadError::ControlCharacter {
                field: "password",
                ..
            })
        ));
        assert!(matches!(
            Wifi::new("a", None, WifiAuth::Wpa, false),
            Err(PayloadError::Inconsistent { .. })
        ));
        assert!(matches!(
            Wifi::new("a", Some("12345678"), WifiAuth::NoPass, false),
            Err(PayloadError::Inconsistent { .. })
        ));
    }

    #[test]
    fn auth_parses_and_displays() {
        for (text, auth) in [
            ("wpa", WifiAuth::Wpa),
            ("WPA2", WifiAuth::Wpa),
            ("wpa3", WifiAuth::Wpa),
            ("wep", WifiAuth::Wep),
            ("nopass", WifiAuth::NoPass),
            ("open", WifiAuth::NoPass),
            ("none", WifiAuth::NoPass),
        ] {
            assert_eq!(text.parse::<WifiAuth>(), Ok(auth));
        }
        assert_eq!(
            "x".parse::<WifiAuth>().unwrap_err().to_string(),
            "unknown authentication 'x': expected wpa, wep or nopass"
        );
        assert_eq!(WifiAuth::default(), WifiAuth::Wpa);
        assert_eq!(WifiAuth::NoPass.to_string(), "nopass");
        assert_eq!(WifiAuth::Wep.to_string(), "wep");
        assert_eq!(WifiAuth::Wpa.to_string(), "wpa");
    }
}
