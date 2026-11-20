//! RFC 3339 date and time input, written out as iCalendar values.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// A calendar date or time, parsed from RFC 3339 text.
///
/// Three forms are accepted:
///
/// | Input | Meaning | iCalendar form |
/// |---|---|---|
/// | `2026-11-20` | an all-day date | `;VALUE=DATE:20261120` |
/// | `2026-11-20T15:30:00Z`, `...+01:00` | an instant, converted to UTC | `:20261120T143000Z` |
/// | `2026-11-20T15:30` | a "floating" wall-clock time (no offset) | `:20261120T153000` |
///
/// Seconds may be left out, fractions of a second are dropped (iCalendar
/// has none), `T` may be a space and `T`/`Z` may be lower case.
///
/// ```
/// use qr_core::payload::DateTime;
/// let t: DateTime = "2026-11-20T15:30:00+01:00".parse()?;
/// assert_eq!(t.to_string(), "20261120T143000Z");
/// # Ok::<(), qr_core::payload::DateTimeError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateTime {
    /// A whole day.
    Date(Date),
    /// A time with no time zone.
    Floating(Date, Time),
    /// An instant in UTC.
    Utc(Date, Time),
}

/// A proleptic Gregorian date in years 1 to 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u32,
    day: u32,
}

/// A time of day to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    hour: u32,
    minute: u32,
    second: u32,
}

/// Why a date or time could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DateTimeError {
    /// The text does not have the RFC 3339 shape.
    #[error("expected RFC 3339, such as 2026-11-20, 2026-11-20T15:30 or 2026-11-20T15:30:00Z")]
    Syntax,
    /// A field is out of range, such as month 13 or 30 February.
    #[error("the {} is out of range", .0.name())]
    Range(Part),
}

impl DateTimeError {
    /// The message as a static string, for [`PayloadError`](super::PayloadError).
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Syntax => {
                "expected RFC 3339, such as 2026-11-20, 2026-11-20T15:30 or 2026-11-20T15:30:00Z"
            }
            Self::Range(Part::Year) => "the year is out of range",
            Self::Range(Part::Month) => "the month is out of range",
            Self::Range(Part::Day) => "the day is out of range",
            Self::Range(Part::Hour) => "the hour is out of range",
            Self::Range(Part::Minute) => "the minute is out of range",
            Self::Range(Part::Second) => "the second is out of range",
            Self::Range(Part::Offset) => "the UTC offset is out of range",
        }
    }
}

/// A field of a date or time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// The year (1 to 9999, also after conversion to UTC).
    Year,
    /// The month.
    Month,
    /// The day of the month.
    Day,
    /// The hour.
    Hour,
    /// The minute.
    Minute,
    /// The second (leap seconds are not accepted).
    Second,
    /// The UTC offset.
    Offset,
}

impl Part {
    /// The lowercase name used in messages.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Day => "day",
            Self::Hour => "hour",
            Self::Minute => "minute",
            Self::Second => "second",
            Self::Offset => "UTC offset",
        }
    }
}

impl DateTime {
    /// Whether this is an all-day date.
    pub const fn is_date(&self) -> bool {
        matches!(self, Self::Date(_))
    }

    /// The iCalendar property suffix: `;VALUE=DATE:` for a date, else `:`.
    pub(crate) const fn value_prefix(&self) -> &'static str {
        if self.is_date() { ";VALUE=DATE:" } else { ":" }
    }

    /// Orders two values of the same form; `None` across forms.
    pub(crate) fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Date(a), Self::Date(b)) => Some(a.cmp(b)),
            (Self::Floating(a, s), Self::Floating(b, t)) | (Self::Utc(a, s), Self::Utc(b, t)) => {
                Some((a, s).cmp(&(b, t)))
            }
            _ => None,
        }
    }
}

impl fmt::Display for DateTime {
    /// The iCalendar `DATE` or `DATE-TIME` value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Date(d) => write!(f, "{d}"),
            Self::Floating(d, t) => write!(f, "{d}T{t}"),
            Self::Utc(d, t) => write!(f, "{d}T{t}Z"),
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}{:02}{:02}", self.hour, self.minute, self.second)
    }
}

impl FromStr for DateTime {
    type Err = DateTimeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let date_part = s.get(..10).ok_or(DateTimeError::Syntax)?;
        let date = parse_date(date_part)?;
        let rest = &s[10..];
        if rest.is_empty() {
            return Ok(Self::Date(date));
        }
        let rest = rest
            .strip_prefix(['T', 't', ' '])
            .ok_or(DateTimeError::Syntax)?;
        let (time, zone) = parse_time(rest)?;
        match zone {
            Zone::Floating => Ok(Self::Floating(date, time)),
            Zone::Offset(minutes) => to_utc(date, time, minutes),
        }
    }
}

enum Zone {
    Floating,
    /// Minutes east of UTC.
    Offset(i64),
}

/// Parses exactly `n` ASCII digits.
fn digits(s: &str, n: usize) -> Result<u32, DateTimeError> {
    if s.len() == n && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().map_err(|_| DateTimeError::Syntax)
    } else {
        Err(DateTimeError::Syntax)
    }
}

/// Splits `s` at byte `at`, failing if it is too short or not on a char
/// boundary.
fn split(s: &str, at: usize) -> Result<(&str, &str), DateTimeError> {
    s.split_at_checked(at).ok_or(DateTimeError::Syntax)
}

fn parse_date(s: &str) -> Result<Date, DateTimeError> {
    let b = s.as_bytes();
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return Err(DateTimeError::Syntax);
    }
    let year = digits(&s[..4], 4)?;
    let month = digits(&s[5..7], 2)?;
    let day = digits(&s[8..10], 2)?;
    let year = i32::try_from(year).map_err(|_| DateTimeError::Syntax)?;
    if year == 0 {
        return Err(DateTimeError::Range(Part::Year));
    }
    if !(1..=12).contains(&month) {
        return Err(DateTimeError::Range(Part::Month));
    }
    if day == 0 || day > days_in_month(year, month) {
        return Err(DateTimeError::Range(Part::Day));
    }
    Ok(Date { year, month, day })
}

fn parse_time(s: &str) -> Result<(Time, Zone), DateTimeError> {
    let (hh, rest) = split(s, 2)?;
    let rest = rest.strip_prefix(':').ok_or(DateTimeError::Syntax)?;
    let (mm, mut rest) = split(rest, 2)?;
    let mut second = 0;
    if let Some(after) = rest.strip_prefix(':') {
        let (ss, after) = split(after, 2)?;
        second = digits(ss, 2)?;
        rest = after;
        if let Some(fraction) = rest.strip_prefix('.') {
            let end = fraction
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(fraction.len());
            if end == 0 {
                return Err(DateTimeError::Syntax);
            }
            rest = &fraction[end..];
        }
    }
    let time = Time {
        hour: digits(hh, 2)?,
        minute: digits(mm, 2)?,
        second,
    };
    if time.hour > 23 {
        return Err(DateTimeError::Range(Part::Hour));
    }
    if time.minute > 59 {
        return Err(DateTimeError::Range(Part::Minute));
    }
    if time.second > 59 {
        return Err(DateTimeError::Range(Part::Second));
    }
    let zone = match rest {
        "" => Zone::Floating,
        "Z" | "z" => Zone::Offset(0),
        _ => {
            let (sign, offset) = split(rest, 1)?;
            let sign = match sign {
                "+" => 1,
                "-" => -1,
                _ => return Err(DateTimeError::Syntax),
            };
            let (oh, om) = split(offset, 2)?;
            let om = om.strip_prefix(':').ok_or(DateTimeError::Syntax)?;
            let (oh, om) = (digits(oh, 2)?, digits(om, 2)?);
            if oh > 23 || om > 59 {
                return Err(DateTimeError::Range(Part::Offset));
            }
            Zone::Offset(sign * i64::from(oh * 60 + om))
        }
    };
    Ok((time, zone))
}

const fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

const fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(date: Date) -> i64 {
    let y = i64::from(date.year) - i64::from(date.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(date.month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(date.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> Result<Date, DateTimeError> {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if !(1..=9999).contains(&year) {
        return Err(DateTimeError::Range(Part::Year));
    }
    let narrow = |v: i64| u32::try_from(v).map_err(|_| DateTimeError::Range(Part::Day));
    Ok(Date {
        year: i32::try_from(year).map_err(|_| DateTimeError::Range(Part::Year))?,
        month: narrow(month)?,
        day: narrow(day)?,
    })
}

/// Converts a local time `offset` minutes east of UTC to UTC.
fn to_utc(date: Date, time: Time, offset: i64) -> Result<DateTime, DateTimeError> {
    let minutes = i64::from(time.hour * 60 + time.minute) - offset;
    let days = days_from_civil(date) + minutes.div_euclid(1_440);
    let minutes = minutes.rem_euclid(1_440);
    let narrow = |v: i64| u32::try_from(v).map_err(|_| DateTimeError::Range(Part::Hour));
    Ok(DateTime::Utc(
        civil_from_days(days)?,
        Time {
            hour: narrow(minutes / 60)?,
            minute: narrow(minutes % 60)?,
            second: time.second,
        },
    ))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn parse(s: &str) -> Result<String, DateTimeError> {
        s.parse::<DateTime>().map(|t| t.to_string())
    }

    #[test]
    fn accepts_the_three_forms() {
        for (input, expected) in [
            ("2026-11-20", "20261120"),
            ("2024-02-29", "20240229"),
            ("2026-11-20T15:30:00Z", "20261120T153000Z"),
            ("2026-11-20t15:30z", "20261120T153000Z"),
            ("2026-11-20 15:30:45.123456Z", "20261120T153045Z"),
            ("2026-11-20T15:30:00+01:00", "20261120T143000Z"),
            ("2026-11-20T00:30:00+01:00", "20261119T233000Z"),
            ("2026-12-31T23:30:00-01:00", "20270101T003000Z"),
            ("2024-02-28T23:00:00-01:00", "20240229T000000Z"),
            ("2026-03-01T00:00:00+00:01", "20260228T235900Z"),
            ("2026-11-20T15:30", "20261120T153000"),
            ("2026-11-20T15:30:07", "20261120T153007"),
        ] {
            assert_eq!(parse(input).as_deref(), Ok(expected), "{input}");
        }
        let date: DateTime = "2026-01-01".parse().unwrap();
        assert!(date.is_date());
        assert_eq!(date.value_prefix(), ";VALUE=DATE:");
        let time: DateTime = "2026-01-01T00:00Z".parse().unwrap();
        assert!(!time.is_date());
        assert_eq!(time.value_prefix(), ":");
    }

    #[test]
    fn rejects_malformed_text() {
        for input in [
            "",
            "2026",
            "2026/11/20",
            "26-11-20",
            "2026-1-20",
            "2026-11-2x",
            "2026-11-20X15:30",
            "2026-11-20T",
            "2026-11-20T1",
            "2026-11-20T15",
            "2026-11-20T15-30",
            "2026-11-20T15:3",
            "2026-11-20T15:30:",
            "2026-11-20T15:30:00.",
            "2026-11-20T15:30:00.5x",
            "2026-11-20T15:30+0100",
            "2026-11-20T15:30+01",
            "2026-11-20T15:30*01:00",
            "2026-11-20T15:30:00ZZ",
            "+026-11-20",
            "2026-11-20T15:3é",
            "２０２６-11-20",
        ] {
            assert_eq!(parse(input), Err(DateTimeError::Syntax), "{input:?}");
        }
    }

    #[test]
    fn rejects_out_of_range_fields() {
        for (input, field) in [
            ("0000-01-01", Part::Year),
            ("2026-00-01", Part::Month),
            ("2026-13-01", Part::Month),
            ("2026-02-29", Part::Day),
            ("2100-02-29", Part::Day),
            ("2026-04-31", Part::Day),
            ("2026-04-00", Part::Day),
            ("2026-01-01T24:00", Part::Hour),
            ("2026-01-01T23:60", Part::Minute),
            ("2026-01-01T23:59:60", Part::Second),
            ("2026-01-01T00:00+24:00", Part::Offset),
            ("2026-01-01T00:00+01:60", Part::Offset),
            ("0001-01-01T00:00+01:00", Part::Year),
            ("9999-12-31T23:00-01:00", Part::Year),
        ] {
            assert_eq!(parse(input), Err(DateTimeError::Range(field)), "{input}");
            let err = DateTimeError::Range(field);
            assert_eq!(err.to_string(), err.reason());
            assert!(err.reason().contains(field.name()));
        }
        assert_eq!(
            DateTimeError::Range(Part::Month).to_string(),
            "the month is out of range"
        );
        assert_eq!(
            DateTimeError::Syntax.to_string(),
            DateTimeError::Syntax.reason()
        );
    }

    #[test]
    fn compares_only_like_forms() {
        let p = |s: &str| s.parse::<DateTime>().unwrap();
        assert_eq!(
            p("2026-01-01").compare(&p("2026-01-02")),
            Some(Ordering::Less)
        );
        assert_eq!(
            p("2026-01-01T10:00").compare(&p("2026-01-01T09:00")),
            Some(Ordering::Greater)
        );
        assert_eq!(
            p("2026-01-01T10:00+01:00").compare(&p("2026-01-01T09:00Z")),
            Some(Ordering::Equal)
        );
        assert_eq!(p("2026-01-01").compare(&p("2026-01-01T00:00")), None);
        assert_eq!(p("2026-01-01T00:00Z").compare(&p("2026-01-01T00:00")), None);
    }

    proptest! {
        #[test]
        fn civil_days_round_trip(days in -719_162i64..=2_932_896) {
            let date = civil_from_days(days).unwrap();
            prop_assert_eq!(days_from_civil(date), days);
        }

        #[test]
        fn offsets_shift_by_exactly_their_minutes(
            day in 0i64..20_000,
            minute in 0i64..1_440,
            offset in -1_439i64..=1_439,
        ) {
            let date = civil_from_days(day).unwrap();
            let time = Time {
                hour: u32::try_from(minute / 60).unwrap(),
                minute: u32::try_from(minute % 60).unwrap(),
                second: 0,
            };
            let DateTime::Utc(d, t) = to_utc(date, time, offset).unwrap() else {
                panic!("not utc");
            };
            let before = days_from_civil(date) * 1_440 + minute;
            let after = days_from_civil(d) * 1_440 + i64::from(t.hour * 60 + t.minute);
            prop_assert_eq!(before - offset, after);
        }

        #[test]
        fn parser_never_panics(s in "\\PC{0,40}") {
            let _ = s.parse::<DateTime>();
        }
    }
}
