//! Escaping round trips for the structured payloads.
//!
//! Each test parses the built payload the way a scanner does (the ZXing
//! grammar for `WIFI:` and `MECARD:`, RFC 2426/5545 unfolding for vCard and
//! iCalendar, RFC 3986 percent-decoding for `mailto:`) and checks that
//! every field comes back exactly, whatever characters it holds.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use qr_core::payload::{Contact, Email, Event, MeCard, Payload, Sms, VCard, Wifi, WifiAuth};
use qr_core::{EncodeOptions, encode};

/// Splits `s` at every `sep` not escaped by a backslash and unescapes each
/// part (ZXing's `WIFI:` / `MECARD:` reading).
fn split_backslashed(s: &str, sep: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            parts
                .last_mut()
                .unwrap()
                .push(chars.next().expect("dangling backslash"));
        } else if c == sep {
            parts.push(String::new());
        } else {
            parts.last_mut().unwrap().push(c);
        }
    }
    parts
}

/// The `KEY:value` fields of a `WIFI:` or `MECARD:` payload, unescaped.
/// The key ends at the first `:`, so a value holding `\:` stays whole.
fn fields(payload: &str, prefix: &str) -> Vec<(String, String)> {
    let body = payload.strip_prefix(prefix).unwrap();
    let body = body.strip_suffix(";;").expect("missing ';;' terminator");
    raw_split(body, ';')
        .into_iter()
        .map(|raw| {
            let (key, value) = raw.split_once(':').expect("field without a key");
            (key.to_owned(), split_backslashed(value, '\u{0}').concat())
        })
        .collect()
}

/// Splits at unescaped `sep` without unescaping.
fn raw_split(s: &str, sep: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let last = parts.last_mut().unwrap();
            last.push(c);
            last.push(chars.next().unwrap());
        } else if c == sep {
            parts.push(String::new());
        } else {
            parts.last_mut().unwrap().push(c);
        }
    }
    parts
}

/// Undoes the vCard/iCalendar TEXT escaping.
fn unescape_text(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next().unwrap() {
                'n' | 'N' => out.push('\n'),
                other => out.push(other),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The value of property `name` in CRLF-separated content lines.
fn property(payload: &str, name: &str) -> Option<String> {
    payload
        .split("\r\n")
        .find_map(|line| line.strip_prefix(&format!("{name}:")))
        .map(unescape_text)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// Printable text, heavy on the characters every format escapes.
fn tricky() -> impl Strategy<Value = String> {
    "([\\\\;,:\"'%&?=#@ /+]|\\PC){1,24}"
}

/// Like [`tricky`], with line breaks in every style.
fn tricky_multiline() -> impl Strategy<Value = String> {
    "([\\\\;,:\"'%&?=#@ /+]|\r\n|\r|\n|\\PC){1,24}"
}

fn normalise_breaks(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

proptest! {
    #[test]
    fn wifi_fields_round_trip(
        ssid in "([\\\\;,:\"]|[a-zA-Z0-9 ]){1,16}",
        password in "([\\\\;,:\"]|\\PC){8,40}",
        hidden: bool,
    ) {
        let wifi = Wifi::new(&ssid, Some(&password), WifiAuth::Wpa, hidden).unwrap();
        let text = wifi.to_string();
        let mut expected = vec![
            ("T".to_owned(), "WPA".to_owned()),
            ("S".to_owned(), ssid),
            ("P".to_owned(), password),
        ];
        if hidden {
            expected.push(("H".to_owned(), "true".to_owned()));
        }
        prop_assert_eq!(fields(&text, "WIFI:"), expected);
    }

    #[test]
    fn mecard_fields_round_trip(
        name in tricky(),
        note in tricky_multiline(),
        url in "[a-z]{1,8}\\.com/[;,:'&?=#@/+a-z%]{0,16}",
    ) {
        let card = MeCard::new(Contact {
            note: Some(note.clone()),
            url: Some(url.clone()),
            ..Contact::new(name.clone())
        });
        // A blank name is refused; anything else must round-trip.
        prop_assume!(card.is_ok());
        let text = card.unwrap().to_string();
        let got = fields(&text, "MECARD:");
        prop_assert_eq!(
            got,
            vec![
                ("N".to_owned(), name.trim().to_owned()),
                ("URL".to_owned(), format!("https://{url}")),
                ("NOTE".to_owned(), note),
            ]
        );
    }

    #[test]
    fn vcard_text_round_trips(
        name in "[a-zA-Z;,\\\\]{1,8}( [a-zA-Z;,\\\\]{1,8}){0,2}",
        org in tricky(),
        note in tricky_multiline(),
    ) {
        let card = VCard::new(Contact {
            org: Some(org.clone()),
            note: Some(note.clone()),
            ..Contact::new(name.clone())
        })
        .unwrap()
        .to_string();
        // Every line is a single content line: no raw breaks inside values.
        for line in card.trim_end_matches("\r\n").split("\r\n") {
            prop_assert!(!line.contains(['\r', '\n']));
        }
        prop_assert_eq!(property(&card, "FN"), Some(name));
        prop_assert_eq!(property(&card, "ORG"), Some(org));
        prop_assert_eq!(property(&card, "NOTE"), Some(normalise_breaks(&note)));
    }

    #[test]
    fn event_text_round_trips(summary in tricky(), location in tricky_multiline()) {
        prop_assume!(!summary.trim().is_empty());
        let event = Event::new(&summary, "2026-11-20T10:00Z", None)
            .and_then(|e| e.with_location(&location))
            .unwrap()
            .to_string();
        prop_assert_eq!(property(&event, "SUMMARY"), Some(summary));
        prop_assert_eq!(property(&event, "LOCATION"), Some(normalise_breaks(&location)));
    }

    #[test]
    fn mailto_parts_round_trip(
        subject in tricky(),
        body in tricky_multiline(),
        local in "[a-z0-9+!$'*,&?=#%/]{1,8}",
    ) {
        let to = format!("{local}@example.com");
        let email = Email::new(&to)
            .and_then(|e| e.with_subject(&subject))
            .and_then(|e| e.with_body(&body))
            .unwrap()
            .to_string();
        let rest = email.strip_prefix("mailto:").unwrap();
        let (address, query) = rest.split_once('?').unwrap();
        prop_assert_eq!(percent_decode(address), to);
        let pairs: Vec<(&str, String)> = query
            .split('&')
            .map(|pair| {
                let (k, v) = pair.split_once('=').unwrap();
                (k, percent_decode(v))
            })
            .collect();
        prop_assert_eq!(&pairs[0], &("subject", subject));
        prop_assert_eq!(&pairs[1], &("body", normalise_breaks(&body).replace('\n', "\r\n")));
    }
}

/// Decodes a bare module grid with `rqrr`.
fn decode(payload: &Payload) -> String {
    let code = encode(payload.to_string().as_bytes(), &EncodeOptions::default()).unwrap();
    let matrix = code.matrix();
    let grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(matrix.width(), |x, y| {
        matrix.is_dark(x, y)
    }));
    let mut data = Vec::new();
    grid.decode_to(&mut data).unwrap();
    String::from_utf8(data).unwrap()
}

#[test]
fn every_payload_kind_encodes_and_decodes() {
    let payloads: Vec<Payload> = vec![
        Wifi::new("Café;Net", Some("p@ss:w,rd\"\\"), WifiAuth::Wpa, true)
            .unwrap()
            .into(),
        VCard::new(Contact {
            phones: vec!["+44 20 7946 0000".into()],
            note: Some("Line 1\nLine 2; done".into()),
            ..Contact::new("Ada Lovelace")
        })
        .unwrap()
        .into(),
        Sms::new("+447700900123", Some("Hello there"))
            .unwrap()
            .into(),
        Event::new(
            "Launch",
            "2026-11-20T15:00:00+01:00",
            Some("2026-11-20T16:00Z"),
        )
        .unwrap()
        .into(),
        Email::new("ada@example.com")
            .and_then(|e| e.with_body("héllo\nworld"))
            .unwrap()
            .into(),
    ];
    for payload in payloads {
        assert_eq!(decode(&payload), payload.to_string(), "{}", payload.kind());
    }
}
