//! Every payload builder with arbitrary fields: none may panic, and every
//! payload that builds must keep its format's invariants (escaped
//! separators, single-line properties, ASCII-only URIs).
//!
//! The input is split at U+001F into the fields each builder takes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qr_core::payload::{
    Contact, DateTime, Email, Event, Geo, MeCard, Sms, Tel, Url, VCard, Wifi, WifiAuth,
};

/// Splits at unescaped `;` and removes the escapes: ZXing's reading of a
/// `WIFI:` or `MECARD:` body.
fn unescape_fields(body: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let escaped = chars.next().expect("dangling backslash");
                parts.last_mut().unwrap().push(escaped);
            }
            ';' => parts.push(String::new()),
            _ => parts.last_mut().unwrap().push(c),
        }
    }
    parts
}

/// A vCard or iCalendar object: CRLF-terminated lines with no other line
/// breaks inside them.
fn assert_content_lines(text: &str) {
    let body = text.strip_suffix("\r\n").expect("missing final CRLF");
    for line in body.split("\r\n") {
        assert!(!line.contains(['\r', '\n']), "raw line break in {line:?}");
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut fields = text.split('\u{1f}');
    let mut next = || fields.next().unwrap_or("");
    let (a, b, c, d) = (next(), next(), next(), next());

    if let Ok(url) = Url::new(a) {
        let s = url.to_string();
        assert!(s.contains("://") && !s.contains(char::is_whitespace));
    }

    let auth = match c.len() % 3 {
        0 => WifiAuth::Wpa,
        1 => WifiAuth::Wep,
        _ => WifiAuth::NoPass,
    };
    let password = (!b.is_empty()).then_some(b);
    if let Ok(wifi) = Wifi::new(a, password, auth, d.is_empty()) {
        let s = wifi.to_string();
        let body = s.strip_prefix("WIFI:").unwrap().strip_suffix(";;").unwrap();
        let parts = unescape_fields(body);
        assert_eq!(parts[0], format!("T:{}", auth.code()));
        assert_eq!(parts[1], format!("S:{a}"));
        if let Some(password) = password {
            assert_eq!(parts[2], format!("P:{password}"));
        }
    }

    let contact = Contact {
        phones: (!b.is_empty()).then(|| b.to_owned()).into_iter().collect(),
        emails: (!c.is_empty()).then(|| c.to_owned()).into_iter().collect(),
        note: Some(d.to_owned()),
        ..Contact::new(a)
    };
    if let Ok(card) = VCard::new(contact.clone()) {
        let s = card.to_string();
        assert!(s.starts_with("BEGIN:VCARD\r\n") && s.ends_with("END:VCARD\r\n"));
        assert_content_lines(&s);
    }
    if let Ok(card) = MeCard::new(contact) {
        let s = card.to_string();
        let body = s
            .strip_prefix("MECARD:")
            .unwrap()
            .strip_suffix(";;")
            .unwrap();
        assert_eq!(unescape_fields(body)[0], format!("N:{}", a.trim()));
    }

    if let Ok(email) = Email::new(a)
        .and_then(|e| e.with_subject(b))
        .and_then(|e| e.with_body(c))
        .and_then(|e| e.with_cc(d))
    {
        let s = email.to_string();
        assert!(s.bytes().all(|byte| byte.is_ascii_graphic()), "{s:?}");
    }

    if let Ok(sms) = Sms::new(a, Some(b)) {
        let s = sms.to_string();
        let number = s.strip_prefix("SMSTO:").unwrap().split(':').next().unwrap();
        assert!(
            number
                .trim_start_matches('+')
                .bytes()
                .all(|x| x.is_ascii_digit())
        );
    }
    if let Ok(tel) = Tel::new(a) {
        assert!(tel.to_string().bytes().all(|x| x.is_ascii_graphic()));
    }

    if let (Ok(lat), Ok(lon)) = (a.parse::<f64>(), b.parse::<f64>())
        && let Ok(geo) = Geo::new(lat, lon)
    {
        let s = geo.to_string();
        let (x, y) = s.strip_prefix("geo:").unwrap().split_once(',').unwrap();
        assert_eq!(
            x.parse::<f64>().unwrap().to_bits(),
            geo.latitude().to_bits()
        );
        assert_eq!(
            y.parse::<f64>().unwrap().to_bits(),
            geo.longitude().to_bits()
        );
    }

    let _ = b.parse::<DateTime>();
    let end = (!c.is_empty()).then_some(c);
    if let Ok(event) = Event::new(a, b, end).and_then(|e| e.with_location(d)) {
        let s = event.to_string();
        assert!(s.starts_with("BEGIN:VEVENT\r\n") && s.ends_with("END:VEVENT\r\n"));
        assert_content_lines(&s);
    }
});
