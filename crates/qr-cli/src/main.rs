//! Entry point for the `qr` binary. All logic lives in the `qr_cli` library
//! so that it can be unit tested without spawning a process.

#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    qr_cli::run(std::env::args_os(), &mut stdout, &mut stderr).into()
}
