//! DECISIONS section 5 (gated): the one-shot path must not start rayon.
//!
//! `qr batch` is the only command that starts worker threads, and every
//! one it starts is counted by `qr_cli::batch::threads_started`. This file
//! holds a single test, so no other test in the same process can start a
//! pool while it runs: the one-shot commands run first and must leave the
//! counter at zero, then a batch must move it (proving the counter works).

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used)]

use qr_cli::{Context, Env, ExitStatus, run};

fn run_args(args: &[&str], stdin: &[u8]) -> ExitStatus {
    let mut stdin = stdin;
    let mut out = Vec::new();
    let mut err = Vec::new();
    run(
        args.iter().copied(),
        &mut Context {
            stdin: &mut stdin,
            stdout: &mut out,
            stderr: &mut err,
            env: Env::default(),
        },
    )
}

#[test]
fn only_batch_starts_worker_threads() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("code.png");
    let png = png.to_str().unwrap();
    let svg = dir.path().join("styled.svg");
    let svg = svg.to_str().unwrap();
    for (args, stdin) in [
        (&["qr", "hello"][..], &b""[..]),
        (&["qr", "-f", "ascii", "--verify"], b"from stdin"),
        (&["qr", "hello", "-o", png], b""),
        (&["qr", "hello", "--shape", "dots", "-o", svg], b""),
        (&["qr", "wifi", "--ssid", "Home", "--auth", "nopass"], b""),
        (&["qr", "decode", png, "--json"], b""),
        (&["qr", "--help"], b""),
    ] {
        assert_eq!(run_args(args, stdin), ExitStatus::Success, "{args:?}");
        assert_eq!(
            qr_cli::batch::threads_started(),
            0,
            "{args:?} started worker threads"
        );
    }

    let out = dir.path().join("out");
    let out = out.to_str().unwrap();
    let status = run_args(
        &["qr", "batch", "-", "--lines", "--out-dir", out, "-j", "2"],
        b"a\nb\n",
    );
    assert_eq!(status, ExitStatus::Success);
    assert_eq!(qr_cli::batch::threads_started(), 2);
}
