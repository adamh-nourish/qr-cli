//! Exact capacity limits: the table matches an independent implementation,
//! every boundary encodes, and one character more is a precise error.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point in this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qr_core::capacity::{data_bits, max_chars};
use qr_core::{
    CapacityError, EcLevel, EncodeError, EncodeOptions, Mode, ModeChoice, Version, VersionChoice,
    encode,
};

const MODES: [Mode; 3] = [Mode::Numeric, Mode::Alphanumeric, Mode::Byte];

fn reference_ec(ec: EcLevel) -> qrcode::EcLevel {
    match ec {
        EcLevel::L => qrcode::EcLevel::L,
        EcLevel::M => qrcode::EcLevel::M,
        EcLevel::Q => qrcode::EcLevel::Q,
        EcLevel::H => qrcode::EcLevel::H,
    }
}

/// A string of `len` characters valid in `mode`.
fn filler(mode: Mode, len: usize) -> Vec<u8> {
    let alphabet: &[u8] = match mode {
        Mode::Numeric => b"0123456789",
        Mode::Alphanumeric => b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:",
        Mode::Byte => b"abcdefghijklmnopqrstuvwxyz",
    };
    (0..len).map(|i| alphabet[i % alphabet.len()]).collect()
}

#[test]
fn data_codeword_table_matches_the_qrcode_crate() {
    for version in Version::MIN.up_to_max() {
        let bits = qrcode::bits::Bits::new(qrcode::Version::Normal(i16::from(version.number())));
        for ec in EcLevel::ALL {
            assert_eq!(
                data_bits(version, ec),
                bits.max_len(reference_ec(ec)).unwrap(),
                "version {version} level {ec}"
            );
        }
    }
}

#[test]
fn every_exact_boundary_encodes_and_one_more_fails() {
    for version in Version::MIN.up_to_max() {
        for ec in EcLevel::ALL {
            for mode in MODES {
                let max = max_chars(mode, version, ec);
                let options = EncodeOptions::default()
                    .with_ec_level(ec)
                    .with_mode(ModeChoice::Fixed(mode))
                    .with_version(VersionChoice::Exact(version));

                let code = encode(&filler(mode, max), &options)
                    .unwrap_or_else(|e| panic!("{mode} v{version}-{ec} at {max}: {e}"));
                assert_eq!(code.version(), version);

                let err = encode(&filler(mode, max + 1), &options).unwrap_err();
                assert_eq!(
                    err,
                    EncodeError::DataTooLong(CapacityError {
                        needed: max + 1,
                        max,
                        mode,
                        ec_level: ec,
                        version,
                    })
                );
            }
        }
    }
}

#[test]
fn auto_version_limits_at_version_40() {
    // (level, numeric, alphanumeric, byte): ISO/IEC 18004 table 7, version 40.
    let limits = [
        (EcLevel::L, 7_089, 4_296, 2_953),
        (EcLevel::M, 5_596, 3_391, 2_331),
        (EcLevel::Q, 3_993, 2_420, 1_663),
        (EcLevel::H, 3_057, 1_852, 1_273),
    ];
    for (ec, numeric, alnum, byte) in limits {
        for (mode, max) in [
            (Mode::Numeric, numeric),
            (Mode::Alphanumeric, alnum),
            (Mode::Byte, byte),
        ] {
            let options = EncodeOptions::default().with_ec_level(ec);
            let code = encode(&filler(mode, max), &options).unwrap();
            assert_eq!(code.version(), Version::MAX);
            assert_eq!(code.mode(), mode);

            match encode(&filler(mode, max + 1), &options) {
                Err(EncodeError::DataTooLong(e)) => {
                    assert_eq!((e.needed, e.max, e.mode), (max + 1, max, mode));
                    assert_eq!((e.ec_level, e.version), (ec, Version::MAX));
                    assert_eq!(e.excess(), 1);
                }
                other => panic!("expected a capacity error, got {other:?}"),
            }
        }
    }
}

#[test]
fn capacity_error_reads_well() {
    let err = encode(
        &vec![b'z'; 3_214],
        &EncodeOptions::default().with_ec_level(EcLevel::L),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "data too long: 3214 bytes exceeds the maximum of 2953 for byte mode at error correction level L (version 40)"
    );
}
