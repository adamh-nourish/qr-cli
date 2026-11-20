//! Arguments of the structured payload subcommands (`qr wifi`, `qr vcard`
//! and the rest). Each one takes its own fields plus every render option
//! of the default command, through [`PayloadOutput`].

use clap::{Args, Subcommand};
use qr_core::payload::WifiAuth;

use super::RenderArgs;

/// The payload subcommands.
#[derive(Debug, Clone, PartialEq, Subcommand)]
pub enum PayloadCommand {
    /// Encode a web link (https:// is added when there is no scheme).
    Url(UrlArgs),
    /// Encode WiFi credentials that phones can join with one tap.
    Wifi(WifiArgs),
    /// Encode a contact card (vCard 3.0).
    Vcard(VCardArgs),
    /// Encode a compact contact card (MeCard).
    Mecard(MeCardArgs),
    /// Encode an email draft (a mailto: link).
    Email(EmailArgs),
    /// Encode a text message draft.
    Sms(SmsArgs),
    /// Encode a phone number to call (a tel: link).
    Tel(TelArgs),
    /// Encode a location (a geo: link).
    Geo(GeoArgs),
    /// Encode a calendar event (iCalendar).
    Event(EventArgs),
}

impl PayloadCommand {
    /// The output options shared by every payload subcommand.
    pub const fn output(&self) -> &PayloadOutput {
        match self {
            Self::Url(a) => &a.output,
            Self::Wifi(a) => &a.output,
            Self::Vcard(a) => &a.output,
            Self::Mecard(a) => &a.output,
            Self::Email(a) => &a.output,
            Self::Sms(a) => &a.output,
            Self::Tel(a) => &a.output,
            Self::Geo(a) => &a.output,
            Self::Event(a) => &a.output,
        }
    }
}

/// Options every payload subcommand takes after its own fields.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct PayloadOutput {
    /// Print the payload text instead of a code (with a trailing newline, so
    /// `... --print-payload | qr` encodes exactly the same data).
    #[arg(
        long,
        conflicts_with_all = ["output", "format", "data_uri", "json", "verify"]
    )]
    pub print_payload: bool,

    /// Output, encoding and styling options, as for the default command.
    #[command(flatten)]
    pub render: RenderArgs,
}

/// `qr url`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct UrlArgs {
    /// The link, such as example.com/page; https:// is added when it has no
    /// scheme.
    #[arg(value_name = "URL")]
    pub url: String,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

/// `qr wifi`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct WifiArgs {
    /// The network name.
    #[arg(long, value_name = "SSID")]
    pub ssid: String,

    /// The password. It is visible in shell history and the process list;
    /// prefer --password-stdin.
    #[arg(long, value_name = "PASSWORD", conflicts_with = "password_stdin")]
    pub password: Option<String>,

    /// Read the password from stdin (one trailing newline is dropped).
    #[arg(long)]
    pub password_stdin: bool,

    /// How the network authenticates: wpa (WPA/WPA2/WPA3), wep or nopass
    /// (an open network).
    #[arg(long, value_name = "AUTH", default_value = "wpa", value_parser = parse_auth)]
    pub auth: WifiAuth,

    /// The network does not broadcast its name.
    #[arg(long)]
    pub hidden: bool,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

fn parse_auth(s: &str) -> Result<WifiAuth, String> {
    s.parse()
        .map_err(|e: qr_core::payload::UnknownWifiAuth| e.to_string())
}

/// Fields shared by `qr vcard` and `qr mecard`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ContactArgs {
    /// The full name, such as 'Ada Lovelace'.
    #[arg(value_name = "NAME")]
    pub name: String,

    /// A phone number (repeat for more).
    #[arg(long = "phone", value_name = "NUMBER")]
    pub phones: Vec<String>,

    /// An email address (repeat for more).
    #[arg(long = "email", value_name = "ADDRESS")]
    pub emails: Vec<String>,

    /// A web site.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// A postal address.
    #[arg(long, value_name = "ADDRESS")]
    pub address: Option<String>,

    /// A note.
    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,
}

/// `qr vcard`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct VCardArgs {
    /// The contact's fields.
    #[command(flatten)]
    pub contact: ContactArgs,

    /// The organisation.
    #[arg(long, value_name = "NAME")]
    pub org: Option<String>,

    /// The job title.
    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

/// `qr mecard`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct MeCardArgs {
    /// The contact's fields.
    #[command(flatten)]
    pub contact: ContactArgs,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

/// `qr email`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct EmailArgs {
    /// The recipient's address.
    #[arg(value_name = "TO")]
    pub to: String,

    /// The subject line.
    #[arg(long, value_name = "TEXT")]
    pub subject: Option<String>,

    /// The message body.
    #[arg(long, value_name = "TEXT")]
    pub body: Option<String>,

    /// A carbon-copy recipient (repeat for more).
    #[arg(long, value_name = "ADDRESS")]
    pub cc: Vec<String>,

    /// A blind carbon-copy recipient (repeat for more).
    #[arg(long, value_name = "ADDRESS")]
    pub bcc: Vec<String>,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

/// `qr sms`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct SmsArgs {
    /// The phone number to text.
    #[arg(value_name = "NUMBER")]
    pub number: String,

    /// The message.
    #[arg(value_name = "MESSAGE")]
    pub message: Option<String>,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

/// `qr tel`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct TelArgs {
    /// The phone number, such as '+44 20 7946 0000'.
    #[arg(value_name = "NUMBER")]
    pub number: String,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

/// `qr geo`.
#[derive(Debug, Clone, PartialEq, Args)]
pub struct GeoArgs {
    /// Latitude in decimal degrees, -90 to 90.
    #[arg(value_name = "LAT", allow_negative_numbers = true)]
    pub latitude: f64,

    /// Longitude in decimal degrees, -180 to 180.
    #[arg(value_name = "LON", allow_negative_numbers = true)]
    pub longitude: f64,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}

/// `qr event`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct EventArgs {
    /// What the event is called.
    #[arg(value_name = "SUMMARY")]
    pub summary: String,

    /// When it starts: 2026-11-20 (all day), 2026-11-20T15:30 (local time)
    /// or 2026-11-20T15:30:00Z / +01:00 (converted to UTC).
    #[arg(long, value_name = "WHEN")]
    pub start: String,

    /// When it ends, in the same form as --start (an all-day end date is
    /// exclusive).
    #[arg(long, value_name = "WHEN")]
    pub end: Option<String>,

    /// Where it takes place.
    #[arg(long, value_name = "PLACE")]
    pub location: Option<String>,

    /// A longer description.
    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,

    /// Output options.
    #[command(flatten)]
    pub output: PayloadOutput,
}
