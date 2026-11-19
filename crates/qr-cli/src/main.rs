//! Entry point for the `qr` binary. All logic lives in the `qr_cli` library
//! so that it can be unit tested without spawning a process.

#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

use qr_cli::{Context, Env};

fn main() -> ExitCode {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut ctx = Context {
        stdin: &mut stdin,
        stdout: &mut stdout,
        stderr: &mut stderr,
        env: Env::detect(),
    };
    qr_cli::run(std::env::args_os(), &mut ctx).into()
}
