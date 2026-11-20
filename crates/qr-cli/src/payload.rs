//! The payload subcommands: turning their arguments into a
//! [`Payload`], reading `--password-stdin` and `--print-payload`.

use std::io::{Read, Write};

use qr_core::payload::{
    Contact, Email, Event, Geo, MeCard, Payload, PayloadError, Sms, Tel, Url, VCard, Wifi,
};
use qr_core::sanitize_lines_for_terminal;

use crate::cli::PayloadCommand;
use crate::cli::payload::ContactArgs;
use crate::error::CliError;
use crate::input::InputSource;

/// The most bytes read for `--password-stdin`. A WPA passphrase is at most
/// 63 characters (64 hex digits), so this leaves room for multi-byte
/// characters while never buffering an unbounded stream.
pub(crate) const MAX_SECRET_BYTES: usize = 1_024;

/// Builds the payload a subcommand describes, reading a secret from
/// `stdin` when one was asked for.
///
/// # Errors
///
/// [`CliError::Payload`] for invalid fields, and the stdin errors of
/// [`read_secret`].
pub(crate) fn build(
    command: &PayloadCommand,
    stdin: &mut dyn Read,
    stdin_is_tty: bool,
) -> Result<Payload, CliError> {
    Ok(match command {
        PayloadCommand::Url(a) => Url::new(&a.url)?.into(),
        PayloadCommand::Wifi(a) => {
            let secret = if a.password_stdin {
                Some(read_secret(stdin, stdin_is_tty)?)
            } else {
                a.password.clone()
            };
            Wifi::new(&a.ssid, secret.as_deref(), a.auth, a.hidden)?.into()
        }
        PayloadCommand::Vcard(a) => VCard::new(Contact {
            org: a.org.clone(),
            title: a.title.clone(),
            ..contact(&a.contact)
        })?
        .into(),
        PayloadCommand::Mecard(a) => MeCard::new(contact(&a.contact))?.into(),
        PayloadCommand::Email(a) => {
            let mut email = Email::new(&a.to)?;
            if let Some(subject) = &a.subject {
                email = email.with_subject(subject)?;
            }
            if let Some(body) = &a.body {
                email = email.with_body(body)?;
            }
            for cc in &a.cc {
                email = email.with_cc(cc)?;
            }
            for bcc in &a.bcc {
                email = email.with_bcc(bcc)?;
            }
            email.into()
        }
        PayloadCommand::Sms(a) => {
            Sms::new(&a.number, a.message.as_deref().or(a.body.as_deref()))?.into()
        }
        PayloadCommand::Tel(a) => Tel::new(&a.number)?.into(),
        PayloadCommand::Geo(a) => Geo::new(a.latitude, a.longitude)?.into(),
        PayloadCommand::Event(a) => {
            let mut event = Event::new(&a.summary, &a.start, a.end.as_deref())?;
            if let Some(location) = &a.location {
                event = event.with_location(location)?;
            }
            if let Some(description) = &a.description {
                event = event.with_description(description)?;
            }
            event.into()
        }
    })
}

fn contact(args: &ContactArgs) -> Contact {
    Contact {
        phones: args.phones.clone(),
        emails: args.emails.clone(),
        url: args.url.clone(),
        address: args.address.clone(),
        note: args.note.clone(),
        ..Contact::new(args.name.clone())
    }
}

/// Reads a secret from stdin: at most [`MAX_SECRET_BYTES`], UTF-8, with one
/// trailing `\n` or `\r\n` dropped. The value never appears in an error.
///
/// # Errors
///
/// [`CliError::SecretStdinIsTerminal`] when stdin is a terminal (typing
/// would echo the secret), [`CliError::ReadInput`] when reading fails, and
/// [`CliError::Payload`] when it is too long or not UTF-8.
pub(crate) fn read_secret(stdin: &mut dyn Read, stdin_is_tty: bool) -> Result<String, CliError> {
    const FIELD: &str = "password";
    if stdin_is_tty {
        return Err(CliError::SecretStdinIsTerminal);
    }
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_SECRET_BYTES + 1).unwrap_or(u64::MAX);
    stdin
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| CliError::ReadInput {
            origin: InputSource::Stdin,
            error,
        })?;
    if bytes.len() > MAX_SECRET_BYTES {
        return Err(PayloadError::InvalidSecret {
            field: FIELD,
            reason: "stdin holds more than 1024 bytes".to_owned(),
            hint: Some("pipe in only the password, followed by at most one newline"),
        }
        .into());
    }
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    if bytes.is_empty() {
        return Err(PayloadError::InvalidSecret {
            field: FIELD,
            reason: "stdin is empty".to_owned(),
            hint: Some(
                "pipe the password in, for example: qr wifi --ssid NAME --password-stdin \
                 < password.txt (or use --auth nopass for an open network)",
            ),
        }
        .into());
    }
    String::from_utf8(bytes).map_err(|_| {
        PayloadError::InvalidSecret {
            field: FIELD,
            reason: "it is not valid UTF-8".to_owned(),
            hint: None,
        }
        .into()
    })
}

/// Writes `payload` for `--print-payload`.
///
/// Off a terminal the bytes are exact, plus one `\n` that `qr` drops again
/// when the text is piped back in. On a terminal, CRLF line ends are shown
/// as plain line breaks and any other control character is escaped.
///
/// # Errors
///
/// [`CliError::Output`] when stdout cannot be written.
pub(crate) fn print(
    payload: &Payload,
    stdout: &mut dyn Write,
    stdout_is_tty: bool,
) -> Result<(), CliError> {
    let text = payload.to_string();
    let shown = if stdout_is_tty {
        let lines = text.replace("\r\n", "\n");
        let mut safe = sanitize_lines_for_terminal(&lines).into_owned();
        if safe.ends_with('\n') {
            safe.pop();
        }
        safe
    } else {
        text
    };
    stdout
        .write_all(shown.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(CliError::Output)
}

#[cfg(test)]
mod tests {
    use std::io;

    use clap::Parser as _;

    use super::*;
    use crate::cli::{Cli, Command};

    fn command(argv: &[&str]) -> PayloadCommand {
        let mut full = vec!["qr"];
        full.extend_from_slice(argv);
        match Cli::try_parse_from(full).unwrap().command {
            Some(Command::Payload(p)) => p,
            other => panic!("not a payload command: {other:?}"),
        }
    }

    fn built(argv: &[&str], stdin: &[u8]) -> Result<String, CliError> {
        build(&command(argv), &mut &stdin[..], false).map(|p| p.to_string())
    }

    #[test]
    fn builds_every_kind() {
        let cases: [(&[&str], &str); 9] = [
            (&["url", "example.com"], "https://example.com"),
            (
                &["wifi", "--ssid", "Home", "--password", "hunter2hunter2"],
                "WIFI:T:WPA;S:Home;P:hunter2hunter2;;",
            ),
            (
                &["vcard", "Ada Lovelace", "--org", "ACME", "--title", "CTO"],
                "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Lovelace;Ada;;;\r\nFN:Ada Lovelace\r\n\
                 ORG:ACME\r\nTITLE:CTO\r\nEND:VCARD\r\n",
            ),
            (
                &[
                    "mecard", "Ada", "--phone", "1", "--email", "a@b", "--url", "x.io",
                ],
                "MECARD:N:Ada;TEL:1;EMAIL:a@b;URL:https\\://x.io;;",
            ),
            (
                &[
                    "email",
                    "a@b",
                    "--subject",
                    "Hi",
                    "--body",
                    "x",
                    "--cc",
                    "c@d",
                    "--bcc",
                    "e@f",
                ],
                "mailto:a@b?subject=Hi&cc=c@d&bcc=e@f&body=x",
            ),
            (&["sms", "123", "hi"], "SMSTO:123:hi"),
            (&["tel", "+1 555"], "tel:+1555"),
            (&["geo", "-33.86", "151.2"], "geo:-33.86,151.2"),
            (
                &[
                    "event",
                    "Lunch",
                    "--start",
                    "2026-11-20T12:00Z",
                    "--end",
                    "2026-11-20T13:00Z",
                    "--location",
                    "Cafe",
                    "--description",
                    "Food",
                ],
                "BEGIN:VEVENT\r\nSUMMARY:Lunch\r\nDTSTART:20261120T120000Z\r\n\
                 DTEND:20261120T130000Z\r\nLOCATION:Cafe\r\nDESCRIPTION:Food\r\nEND:VEVENT\r\n",
            ),
        ];
        for (argv, expected) in cases {
            assert_eq!(built(argv, b"").unwrap(), expected, "{argv:?}");
        }
        assert_eq!(
            built(
                &["mecard", "Ada", "--address", "1 Road", "--note", "n"],
                b""
            )
            .unwrap(),
            "MECARD:N:Ada;ADR:1 Road;NOTE:n;;"
        );
    }

    #[test]
    fn password_stdin_drops_one_newline() {
        let argv = ["wifi", "--ssid", "Home", "--password-stdin"];
        for input in [&b"pass;word\n"[..], b"pass;word\r\n", b"pass;word"] {
            assert_eq!(
                built(&argv, input).unwrap(),
                "WIFI:T:WPA;S:Home;P:pass\\;word;;"
            );
        }
    }

    #[test]
    fn secrets_are_bounded_and_checked() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("boom"))
            }
        }

        let tty = read_secret(&mut &b"x"[..], true).unwrap_err();
        assert!(matches!(tty, CliError::SecretStdinIsTerminal));
        let long = vec![b'a'; MAX_SECRET_BYTES + 1];
        let err = read_secret(&mut &long[..], false).unwrap_err();
        assert!(err.to_string().contains("more than 1024"), "{err}");
        let exact = vec![b'a'; MAX_SECRET_BYTES];
        assert_eq!(read_secret(&mut &exact[..], false).unwrap().len(), 1_024);
        let err = read_secret(&mut &b"\xff\xfe"[..], false).unwrap_err();
        assert_eq!(err.to_string(), "invalid password: it is not valid UTF-8");

        let err = read_secret(&mut Broken, false).unwrap_err();
        assert!(matches!(err, CliError::ReadInput { .. }), "{err}");
    }

    #[test]
    fn invalid_fields_are_payload_errors() {
        for argv in [
            &["url", "a b"][..],
            &["wifi", "--ssid", "x"],
            &["vcard", " "],
            &["mecard", " "],
            &["email", "nobody"],
            &["email", "a@b", "--subject", "a\nb"],
            &["email", "a@b", "--body", "\u{7}"],
            &["email", "a@b", "--cc", "x"],
            &["email", "a@b", "--bcc", "x"],
            &["sms", "abc"],
            &["tel", "abc"],
            &["geo", "91", "0"],
            &["event", "x", "--start", "nope"],
            &["event", "x", "--start", "2026-01-01", "--location", "\u{7}"],
            &[
                "event",
                "x",
                "--start",
                "2026-01-01",
                "--description",
                "\u{7}",
            ],
        ] {
            let err = built(argv, b"").unwrap_err();
            assert!(matches!(err, CliError::Payload(_)), "{argv:?}: {err}");
        }
    }

    #[test]
    fn print_is_exact_off_a_terminal_and_safe_on_one() {
        // A lone CR is data a terminal would act on (it rewinds the line).
        let payload: Payload = Sms::new("1", Some("a\r\nb\rc")).unwrap().into();
        let mut out = Vec::new();
        print(&payload, &mut out, false).unwrap();
        assert_eq!(out, "SMSTO:1:a\r\nb\rc\n".as_bytes());
        let mut out = Vec::new();
        print(&payload, &mut out, true).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "SMSTO:1:a\nb\\u{d}c\n");
        let card: Payload = VCard::new(Contact::new("A")).unwrap().into();
        let mut out = Vec::new();
        print(&card, &mut out, true).unwrap();
        assert!(String::from_utf8(out).unwrap().ends_with("END:VCARD\n"));
    }

    #[test]
    fn print_reports_write_failures() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let payload: Payload = Tel::new("1").unwrap().into();
        assert!(matches!(
            print(&payload, &mut Closed, false),
            Err(CliError::Output(_))
        ));
    }
}
