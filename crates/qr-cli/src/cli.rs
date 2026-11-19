//! Command-line argument definitions.
//!
//! The v1 surface (generate options, payload subcommands, `decode`, `batch`,
//! `completions` and `man`) is added here incrementally. Keeping the clap
//! types in one module lets completions and the man page be generated from a
//! single source of truth.

use clap::Parser;

/// Generate QR codes from text, files or structured payloads.
#[derive(Debug, Parser)]
#[command(
    name = "qr",
    version,
    about,
    long_about = None,
    arg_required_else_help = true
)]
pub struct Cli {}
