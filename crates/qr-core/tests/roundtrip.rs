//! Round-trip tests: every symbol the encoder produces must decode, through
//! the independent `rqrr` decoder, to exactly the input bytes, with the
//! version, error correction level and mask the encoder reports.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point in this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use qr_core::{
    EcLevel, EncodeOptions, Mask, Matrix, Mode, ModeChoice, QrCode, Version, VersionChoice, encode,
};

/// What `rqrr` read back from a symbol.
#[derive(Debug)]
struct Decoded {
    version: usize,
    ec_level: EcLevel,
    mask: u8,
    data: Vec<u8>,
}

/// Decodes a bare module grid, skipping image detection.
fn decode_grid(matrix: &Matrix) -> Decoded {
    let w = matrix.width();
    let grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(w, |x, y| matrix.is_dark(x, y)));
    let mut data = Vec::new();
    let meta = grid.decode_to(&mut data).expect("rqrr failed to decode");
    Decoded {
        version: meta.version.0,
        // Format information encodes the level as M=0, L=1, H=2, Q=3.
        ec_level: [EcLevel::M, EcLevel::L, EcLevel::H, EcLevel::Q][usize::from(meta.ecc_level)],
        mask: u8::try_from(meta.mask).unwrap(),
        data,
    }
}

/// Rasterises the matrix with a quiet zone and runs full image detection, so
/// orientation and the quiet zone semantics of `Matrix::is_dark` are tested.
fn decode_image(matrix: &Matrix, margin: usize, scale: usize) -> Vec<u8> {
    let side = (matrix.width() + 2 * margin) * scale;
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| {
        let (mx, my) = (x / scale, y / scale);
        let dark = mx >= margin && my >= margin && matrix.is_dark(mx - margin, my - margin);
        if dark { 0 } else { 255 }
    });
    let grids = img.detect_grids();
    assert_eq!(grids.len(), 1, "expected exactly one symbol");
    let mut data = Vec::new();
    grids[0]
        .decode_to(&mut data)
        .expect("rqrr failed to decode");
    data
}

fn assert_round_trip(data: &[u8], code: &QrCode) {
    let decoded = decode_grid(code.matrix());
    assert_eq!(decoded.data, data);
    assert_eq!(decoded.version, usize::from(code.version().number()));
    assert_eq!(decoded.ec_level, code.ec_level());
    assert_eq!(decoded.mask, code.mask().number());
}

fn ec_level() -> impl Strategy<Value = EcLevel> {
    prop::sample::select(EcLevel::ALL.to_vec())
}

fn mask() -> impl Strategy<Value = Option<Mask>> {
    prop::option::of((0u8..=7).prop_map(|n| Mask::new(n).unwrap()))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn arbitrary_bytes_round_trip(
        data in prop::collection::vec(any::<u8>(), 0..600),
        ec in ec_level(),
        mask in mask(),
    ) {
        let options = EncodeOptions::default().with_ec_level(ec).with_mask(mask);
        let code = encode(&data, &options).unwrap();
        prop_assert_eq!(code.mode(), Mode::narrowest_for(&data));
        if let Some(forced) = mask {
            prop_assert_eq!(code.mask(), forced);
        }
        assert_round_trip(&data, &code);
    }

    #[test]
    fn numeric_strings_round_trip(data in "[0-9]{0,1500}", ec in ec_level(), mask in mask()) {
        let options = EncodeOptions::default()
            .with_ec_level(ec)
            .with_mode(ModeChoice::Fixed(Mode::Numeric))
            .with_mask(mask);
        let code = encode(data.as_bytes(), &options).unwrap();
        prop_assert_eq!(code.mode(), Mode::Numeric);
        assert_round_trip(data.as_bytes(), &code);
    }

    #[test]
    fn alphanumeric_strings_round_trip(
        data in "[0-9A-Z $%*+./:-]{0,900}",
        ec in ec_level(),
        mask in mask(),
    ) {
        let options = EncodeOptions::default()
            .with_ec_level(ec)
            .with_mode(ModeChoice::Fixed(Mode::Alphanumeric))
            .with_mask(mask);
        let code = encode(data.as_bytes(), &options).unwrap();
        assert_round_trip(data.as_bytes(), &code);
    }

    #[test]
    fn minimum_version_is_respected(
        data in prop::collection::vec(any::<u8>(), 0..100),
        min in 1u8..=40,
    ) {
        let min = Version::new(min).unwrap();
        let options = EncodeOptions::default().with_version(VersionChoice::AtLeast(min));
        let code = encode(&data, &options).unwrap();
        prop_assert!(code.version() >= min);
        prop_assert_eq!(code.matrix().width(), code.version().width());
        assert_round_trip(&data, &code);
    }

    #[test]
    fn encoding_is_deterministic(data in prop::collection::vec(any::<u8>(), 0..200)) {
        let options = EncodeOptions::default();
        prop_assert_eq!(encode(&data, &options).unwrap(), encode(&data, &options).unwrap());
    }
}

#[test]
fn every_version_and_mask_round_trips() {
    let data = b"qr-cli/rt";
    for version in Version::MIN
        .up_to_max()
        .filter(|v| v.number() % 3 == 1 || v.number() == 40)
    {
        for mask in Mask::all() {
            let options = EncodeOptions::default()
                .with_version(VersionChoice::Exact(version))
                .with_mask(Some(mask));
            let code = encode(data, &options).unwrap();
            assert_eq!(code.version(), version);
            assert_eq!(code.mask(), mask);
            assert_round_trip(data, &code);
        }
    }
}

#[test]
fn empty_input_encodes_to_a_version_1_symbol() {
    let code = encode(b"", &EncodeOptions::default()).unwrap();
    assert_eq!(code.version(), Version::MIN);
    assert_round_trip(b"", &code);
}

#[test]
fn rendered_images_are_detected_in_the_right_orientation() {
    let cases: [&[u8]; 3] = [
        b"HELLO WORLD",
        "unicode: \u{1F600} caf\u{e9}".as_bytes(),
        &[b'7'; 300],
    ];
    for data in cases {
        for ec in EcLevel::ALL {
            let code = encode(data, &EncodeOptions::default().with_ec_level(ec)).unwrap();
            assert_eq!(decode_image(code.matrix(), 4, 4), data);
        }
    }
}

#[test]
fn large_symbol_is_detected_in_an_image() {
    let data = vec![b'q'; 2_000];
    let code = encode(&data, &EncodeOptions::default().with_ec_level(EcLevel::L)).unwrap();
    assert_eq!(code.version().number(), 33);
    assert_eq!(decode_image(code.matrix(), 4, 3), data);
}
