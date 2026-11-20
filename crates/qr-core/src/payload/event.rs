//! Calendar events as an iCalendar `VEVENT` (RFC 5545).

use std::cmp::Ordering;
use std::fmt;

use super::escape::{self, Controls};
use super::{DateTime, PayloadError};

/// A calendar event:
///
/// ```text
/// BEGIN:VEVENT
/// SUMMARY:<summary>
/// DTSTART:<start>
/// DTEND:<end>
/// LOCATION:<location>
/// DESCRIPTION:<description>
/// END:VEVENT
/// ```
///
/// This is the bare `VEVENT` that ZXing and phone cameras read, not a full
/// `VCALENDAR` object: that would need a `UID` and a `DTSTAMP`, which make
/// the code larger and different on every run. Text values escape
/// `\ ; ,` and line breaks; lines end in CRLF and are not folded.
///
/// Times are RFC 3339 text (see [`DateTime`]): an offset is converted to
/// UTC, a time without one stays a floating local time, and a bare date
/// makes an all-day event whose end date is exclusive.
///
/// ```
/// use qr_core::payload::Event;
/// let event = Event::new("Launch", "2026-11-20T15:00:00+01:00", Some("2026-11-20T16:00:00+01:00"))?
///     .with_location("Room 1, Floor 2")?;
/// assert_eq!(
///     event.to_string(),
///     "BEGIN:VEVENT\r\nSUMMARY:Launch\r\nDTSTART:20261120T140000Z\r\n\
///      DTEND:20261120T150000Z\r\nLOCATION:Room 1\\, Floor 2\r\nEND:VEVENT\r\n"
/// );
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Event {
    summary: String,
    start: DateTime,
    end: Option<DateTime>,
    location: Option<String>,
    description: Option<String>,
}

impl Event {
    /// Validates the summary and times.
    ///
    /// # Errors
    ///
    /// [`PayloadError`] when the summary is blank or holds a control
    /// character other than a tab or line break, a time is not RFC 3339,
    /// the start and end are of different forms (date, UTC or floating), or
    /// the end is not after the start.
    pub fn new(summary: &str, start: &str, end: Option<&str>) -> Result<Self, PayloadError> {
        escape::check_required("summary", summary, Controls::AllowLineBreaks)?;
        let start_time = parse_time("start", start)?;
        let end_time = end.map(|e| parse_time("end", e)).transpose()?;
        if let Some(end_time) = &end_time {
            match start_time.compare(end_time) {
                Some(Ordering::Less) => {}
                Some(_) if start_time.is_date() => {
                    return Err(PayloadError::inconsistent(
                        "the event must end after it starts",
                        Some("an all-day end date is exclusive: a one-day event ends the next day"),
                    ));
                }
                Some(_) => {
                    return Err(PayloadError::inconsistent(
                        "the event must end after it starts",
                        None,
                    ));
                }
                None => {
                    return Err(PayloadError::inconsistent(
                        "the start and end must both be dates, both have a UTC offset \
                         or both have none",
                        Some("for example --start 2026-11-20T15:00 --end 2026-11-20T16:00"),
                    ));
                }
            }
        }
        Ok(Self {
            summary: summary.to_owned(),
            start: start_time,
            end: end_time,
            location: None,
            description: None,
        })
    }

    /// Sets where the event takes place.
    ///
    /// # Errors
    ///
    /// [`PayloadError::ControlCharacter`] for a control character other
    /// than a tab or line break.
    pub fn with_location(mut self, location: &str) -> Result<Self, PayloadError> {
        escape::check_controls("location", location, Controls::AllowLineBreaks)?;
        self.location = Some(location.to_owned());
        Ok(self)
    }

    /// Sets a longer description.
    ///
    /// # Errors
    ///
    /// As [`Event::with_location`].
    pub fn with_description(mut self, description: &str) -> Result<Self, PayloadError> {
        escape::check_controls("description", description, Controls::AllowLineBreaks)?;
        self.description = Some(description.to_owned());
        Ok(self)
    }
}

fn parse_time(field: &'static str, value: &str) -> Result<DateTime, PayloadError> {
    value
        .parse()
        .map_err(|err: super::DateTimeError| PayloadError::Invalid {
            field,
            value: value.to_owned(),
            reason: err.reason(),
            hint: None,
        })
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BEGIN:VEVENT\r\nSUMMARY:{}\r\nDTSTART{}{}\r\n",
            escape::text(&self.summary),
            self.start.value_prefix(),
            self.start
        )?;
        if let Some(end) = &self.end {
            write!(f, "DTEND{}{end}\r\n", end.value_prefix())?;
        }
        if let Some(location) = &self.location {
            write!(f, "LOCATION:{}\r\n", escape::text(location))?;
        }
        if let Some(description) = &self.description {
            write!(f, "DESCRIPTION:{}\r\n", escape::text(description))?;
        }
        f.write_str("END:VEVENT\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_every_field() {
        let event = Event::new("Q4; review, all", "2026-11-20", Some("2026-11-22"))
            .and_then(|e| e.with_location("HQ"))
            .and_then(|e| e.with_description("Bring\nlaptops \\ notes"))
            .unwrap();
        assert_eq!(
            event.to_string(),
            "BEGIN:VEVENT\r\nSUMMARY:Q4\\; review\\, all\r\nDTSTART;VALUE=DATE:20261120\r\n\
             DTEND;VALUE=DATE:20261122\r\nLOCATION:HQ\r\n\
             DESCRIPTION:Bring\\nlaptops \\\\ notes\r\nEND:VEVENT\r\n"
        );
        let floating = Event::new("x", "2026-11-20T09:00", None).unwrap();
        assert_eq!(
            floating.to_string(),
            "BEGIN:VEVENT\r\nSUMMARY:x\r\nDTSTART:20261120T090000\r\nEND:VEVENT\r\n"
        );
    }

    #[test]
    fn rejects_bad_times_and_orders() {
        let err = |start: &str, end: Option<&str>| Event::new("x", start, end).unwrap_err();
        assert!(matches!(
            err("tomorrow", None),
            PayloadError::Invalid { field: "start", .. }
        ));
        assert!(
            err("2026-01-01", Some("soon"))
                .to_string()
                .contains("RFC 3339")
        );
        for (input, part) in [
            ("0000-01-01", "year"),
            ("2026-13-01", "month"),
            ("2026-02-30", "day"),
            ("2026-01-01T24:00", "hour"),
            ("2026-01-01T00:60", "minute"),
            ("2026-01-01T00:00:60", "second"),
            ("2026-01-01T00:00+30:00", "UTC offset"),
        ] {
            let message = err(input, None).to_string();
            assert!(
                message.contains(&format!("the {part} is out of range")),
                "{message}"
            );
        }
        assert!(
            err("2026-01-02", Some("2026-01-02"))
                .hint()
                .is_some_and(|h| h.contains("exclusive"))
        );
        assert_eq!(
            err("2026-01-01T10:00Z", Some("2026-01-01T09:00Z")).to_string(),
            "the event must end after it starts"
        );
        assert!(
            err("2026-01-01", Some("2026-01-02T00:00Z"))
                .to_string()
                .contains("both")
        );
        assert!(
            err("2026-01-01T00:00", Some("2026-01-02T00:00Z"))
                .to_string()
                .contains("both")
        );
        assert_eq!(
            Event::new(" ", "2026-01-01", None),
            Err(PayloadError::Empty { field: "summary" })
        );
        let event = Event::new("x", "2026-01-01", None).unwrap();
        assert!(event.clone().with_location("\u{7}").is_err());
        assert!(event.with_description("\u{1b}").is_err());
    }
}
