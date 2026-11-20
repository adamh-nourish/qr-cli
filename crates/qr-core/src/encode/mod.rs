//! Turning data into a [`QrCode`].
//!
//! Encoding is split in two so that backends stay interchangeable:
//!
//! 1. [`plan`] validates the data against the requested mode, splits it
//!    into [`Segment`]s and picks the version. It owns every user-facing
//!    decision, including the exact capacity error, so all backends report
//!    limits identically.
//! 2. A [`QrEncoder`] backend builds the module matrix for that plan.
//!
//! [`encode`] runs both with the default backend, [`QrcodeEncoder`].

mod format_info;
mod qrcode_backend;
mod segment;

pub use self::qrcode_backend::QrcodeEncoder;
pub use self::segment::Segment;

use crate::capacity::{count_indicator_bits, data_bits, fits, max_chars};
use crate::error::{CapacityError, CapacityUnit, EncodeError};
use crate::matrix::Matrix;
use crate::options::{EcLevel, EncodeOptions, Mask, Mode, ModeChoice, Version};

/// Something that turns bytes into a QR code symbol.
///
/// Implementations must be deterministic: the same data and options always
/// produce the same symbol. `Send + Sync` lets one encoder serve a batch
/// across threads.
pub trait QrEncoder: Send + Sync {
    /// Encodes `data` as a single QR code symbol.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::DataTooLong`] when the data does not fit the
    /// largest allowed version, and [`EncodeError::InvalidCharacter`] when a
    /// fixed mode cannot represent the data.
    fn encode(&self, data: &[u8], options: &EncodeOptions) -> Result<QrCode, EncodeError>;
}

/// The concrete parameters for one symbol, as chosen by [`plan`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EncodePlan {
    /// The segments, in input order, covering the whole input. A fixed
    /// mode gives exactly one segment (even for empty input); automatic
    /// mode gives none for empty input.
    pub segments: Vec<Segment>,
    /// The symbol version, the smallest that fits within the options.
    pub version: Version,
    /// The error correction level.
    pub ec_level: EcLevel,
    /// A forced mask, or `None` to let the backend choose the
    /// lowest-penalty mask (ISO/IEC 18004 section 7.8.3).
    pub mask: Option<Mask>,
}

impl EncodePlan {
    /// The widest mode among the segments: the single mode for single-mode
    /// data, byte mode for anything containing bytes outside the
    /// alphanumeric set. [`Mode::Numeric`] for empty input.
    pub fn mode(&self) -> Mode {
        segment::widest_mode(&self.segments)
    }

    /// Data bits the segments take at the planned version, including mode
    /// and character count headers but not the terminator.
    pub fn bits(&self) -> usize {
        segment::total_bits(&self.segments, self.version)
    }
}

/// Validates `data` against `options` and chooses the segments and version.
///
/// Automatic mode uses mixed-mode segmentation (see [`ModeChoice::Auto`]):
/// for each candidate version the data is segmented for that version's
/// character count widths, and the first version whose data capacity holds
/// the segments is chosen. This is the `qrcode` crate's sizing, except
/// that Kanji mode is never used and that a single segment in the
/// narrowest mode is also tried at each version, so the version is never
/// larger than the narrowest fixed mode would give.
///
/// # Errors
///
/// [`EncodeError::InvalidCharacter`] if a fixed mode cannot represent a
/// byte, and [`EncodeError::DataTooLong`] with the exact limit at the
/// largest allowed version: in characters for single-mode data, in data
/// bits for mixed data ([`CapacityUnit`]).
///
/// ```
/// use qr_core::{EncodeError, EncodeOptions, EcLevel, Mode, plan};
///
/// let p = plan(b"HELLO WORLD", &EncodeOptions::default()).unwrap();
/// assert_eq!((p.mode(), p.version.number()), (Mode::Alphanumeric, 1));
///
/// // A phone number is a short byte segment and a numeric one.
/// let p = plan(b"tel:+441234567890", &EncodeOptions::default()).unwrap();
/// assert_eq!((p.segments.len(), p.version.number()), (2, 1));
///
/// let too_long = vec![b'a'; 2_954];
/// let options = EncodeOptions::default().with_ec_level(EcLevel::L);
/// match plan(&too_long, &options) {
///     Err(EncodeError::DataTooLong(e)) => assert_eq!((e.needed, e.max), (2_954, 2_953)),
///     other => panic!("unexpected {other:?}"),
/// }
/// ```
pub fn plan(data: &[u8], options: &EncodeOptions) -> Result<EncodePlan, EncodeError> {
    let (segments, version) = match options.mode {
        ModeChoice::Fixed(mode) => plan_fixed(data, mode, *options)?,
        ModeChoice::Auto => plan_auto(data, *options)?,
    };
    Ok(EncodePlan {
        segments,
        version,
        ec_level: options.ec_level,
        mask: options.mask,
    })
}

/// One segment in `mode`, at the smallest version that holds it.
fn plan_fixed(
    data: &[u8],
    mode: Mode,
    options: EncodeOptions,
) -> Result<(Vec<Segment>, Version), EncodeError> {
    if let Some(offset) = data.iter().position(|&b| !mode.accepts(b)) {
        return Err(EncodeError::InvalidCharacter {
            mode,
            offset,
            byte: data[offset],
        });
    }
    let (min, max, ec) = (
        options.version.min(),
        options.version.max(),
        options.ec_level,
    );
    let version = versions(min, max)
        .find(|&v| fits(mode, v, ec, data.len()))
        .ok_or_else(|| chars_error(data.len(), mode, ec, max))?;
    let segment = Segment {
        mode,
        start: 0,
        end: data.len(),
    };
    Ok((vec![segment], version))
}

/// Segmented data at the smallest version that holds it.
///
/// The greedy merge in [`segment::optimize`] only compares each run with
/// its left neighbour, so it can need more bits than packing the whole
/// input as one segment in its narrowest mode (`XSL790106502KNC+` takes
/// 109 bits greedily and 101 as one alphanumeric segment). Each version
/// therefore also tries that single segment, so automatic mode never needs
/// a larger version than the narrowest fixed mode. The greedy segments are
/// preferred whenever they fit, which keeps the symbol bit-identical to
/// `qrcode`'s whenever both pick the same version.
fn plan_auto(data: &[u8], options: EncodeOptions) -> Result<(Vec<Segment>, Version), EncodeError> {
    let (min, max, ec) = (
        options.version.min(),
        options.version.max(),
        options.ec_level,
    );
    let runs = segment::runs(data);
    let single = single_segment(data);
    // Segmentation only changes where the count widths do (versions 10 and
    // 27), so it is computed at most once per width class.
    let mut by_class: [Option<Vec<Segment>>; 3] = Default::default();
    for version in versions(min, max) {
        let segments =
            by_class[count_class(version)].get_or_insert_with(|| segment::optimize(&runs, version));
        if segments_fit(segments, version, ec) {
            return Ok((segments.clone(), version));
        }
        if segments_fit(&single, version, ec) {
            return Ok((single, version));
        }
    }
    let greedy = by_class[count_class(max)]
        .get_or_insert_with(|| segment::optimize(&runs, max))
        .as_slice();
    // Report whichever needs fewer bits at the largest version, so the
    // error states the least the data needs.
    let segments = if segment::total_bits(&single, max) <= segment::total_bits(greedy, max) {
        single.as_slice()
    } else {
        greedy
    };
    let error = match segments {
        // One segment is measured in its own characters, exactly as a
        // fixed mode would be.
        [] => chars_error(0, Mode::Numeric, ec, max),
        [single] => chars_error(single.len(), single.mode, ec, max),
        _ => CapacityError {
            needed: segment::total_bits(segments, max),
            max: data_bits(max, ec),
            unit: CapacityUnit::Bits,
            mode: segment::widest_mode(segments),
            ec_level: ec,
            version: max,
            bytes: data.len(),
        },
    };
    Err(error.into())
}

/// All of `data` as one segment in its narrowest mode, or no segments for
/// empty input (as [`segment::optimize`] gives).
fn single_segment(data: &[u8]) -> Vec<Segment> {
    if data.is_empty() {
        return Vec::new();
    }
    vec![Segment {
        mode: Mode::narrowest_for(data),
        start: 0,
        end: data.len(),
    }]
}

/// Whether `segments` fit a symbol of `version` at `ec`: every count fits
/// its field and the total fits the data capacity.
fn segments_fit(segments: &[Segment], version: Version, ec: EcLevel) -> bool {
    let counts_fit = segments
        .iter()
        .all(|s| s.len() < 1 << count_indicator_bits(s.mode, version));
    counts_fit && segment::total_bits(segments, version) <= data_bits(version, ec)
}

/// Which character count width class `version` is in.
fn count_class(version: Version) -> usize {
    match version.number() {
        1..=9 => 0,
        10..=26 => 1,
        _ => 2,
    }
}

fn versions(min: Version, max: Version) -> impl Iterator<Item = Version> {
    min.up_to_max().take_while(move |v| *v <= max)
}

fn chars_error(len: usize, mode: Mode, ec: EcLevel, max: Version) -> CapacityError {
    CapacityError {
        needed: len,
        max: max_chars(mode, max, ec),
        unit: CapacityUnit::Characters,
        mode,
        ec_level: ec,
        version: max,
        bytes: len,
    }
}

/// Encodes `data` with the default encoder.
///
/// # Errors
///
/// See [`QrEncoder::encode`].
///
/// ```
/// use qr_core::{EncodeOptions, encode};
/// let code = encode(b"https://example.com", &EncodeOptions::default()).unwrap();
/// assert_eq!(code.version().number(), 2);
/// assert_eq!(code.matrix().width(), 25);
/// ```
pub fn encode(data: &[u8], options: &EncodeOptions) -> Result<QrCode, EncodeError> {
    QrcodeEncoder.encode(data, options)
}

/// An encoded symbol: the module matrix plus the parameters it was built
/// with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QrCode {
    matrix: Matrix,
    version: Version,
    ec_level: EcLevel,
    segments: Vec<Segment>,
    mask: Mask,
}

impl QrCode {
    /// Assembles a symbol from a backend's matrix and the plan it followed.
    ///
    /// # Errors
    ///
    /// [`EncodeError::Internal`] if the matrix width does not match the
    /// planned version, which would mean the backend ignored the plan.
    pub fn from_parts(matrix: Matrix, plan: &EncodePlan, mask: Mask) -> Result<Self, EncodeError> {
        if matrix.width() != plan.version.width() {
            return Err(EncodeError::Internal(format!(
                "backend produced a {w}x{w} matrix for version {v}",
                w = matrix.width(),
                v = plan.version
            )));
        }
        if plan.mask.is_some_and(|forced| forced != mask) {
            return Err(EncodeError::Internal(format!(
                "backend used mask {mask} instead of the forced mask"
            )));
        }
        Ok(Self {
            matrix,
            version: plan.version,
            ec_level: plan.ec_level,
            segments: plan.segments.clone(),
            mask,
        })
    }

    /// The module grid, without a quiet zone.
    pub fn matrix(&self) -> &Matrix {
        &self.matrix
    }

    /// Consumes the symbol, returning its module grid.
    pub fn into_matrix(self) -> Matrix {
        self.matrix
    }

    /// The symbol version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// The error correction level.
    pub fn ec_level(&self) -> EcLevel {
        self.ec_level
    }

    /// The data mode: the widest mode among [`QrCode::segments`] (see
    /// [`EncodePlan::mode`]).
    pub fn mode(&self) -> Mode {
        segment::widest_mode(&self.segments)
    }

    /// The segments the data was packed in, in input order.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Whether the data was split into segments of more than one mode.
    pub fn is_mixed_mode(&self) -> bool {
        self.segments.windows(2).any(|w| w[0].mode != w[1].mode)
    }

    /// The mask pattern that was applied: the forced mask, or the one the
    /// backend chose, as recorded in the symbol's format information.
    pub fn mask(&self) -> Mask {
        self.mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::VersionChoice;

    fn v(n: u8) -> Version {
        Version::new(n).unwrap()
    }

    #[test]
    fn auto_mode_picks_the_narrowest_mode() {
        let o = EncodeOptions::default();
        assert_eq!(plan(b"12345", &o).unwrap().mode(), Mode::Numeric);
        assert_eq!(plan(b"AB-12", &o).unwrap().mode(), Mode::Alphanumeric);
        assert_eq!(plan(b"ab-12", &o).unwrap().mode(), Mode::Byte);
        assert_eq!(plan(b"", &o).unwrap().mode(), Mode::Numeric);
        assert_eq!(plan(b"", &o).unwrap().segments, Vec::<Segment>::new());
    }

    #[test]
    fn fixed_mode_rejects_the_first_bad_byte() {
        let o = EncodeOptions::default().with_mode(ModeChoice::Fixed(Mode::Numeric));
        assert_eq!(
            plan(b"12a4b", &o),
            Err(EncodeError::InvalidCharacter {
                mode: Mode::Numeric,
                offset: 2,
                byte: b'a'
            })
        );
        let o = o.with_mode(ModeChoice::Fixed(Mode::Byte));
        assert_eq!(plan(b"12a4b", &o).unwrap().mode(), Mode::Byte);
    }

    #[test]
    fn version_choice_is_honoured() {
        let base = EncodeOptions::default();
        assert_eq!(plan(b"x", &base).unwrap().version, v(1));
        let at_least = base.with_version(VersionChoice::AtLeast(v(10)));
        assert_eq!(plan(b"x", &at_least).unwrap().version, v(10));
        let exact = base.with_version(VersionChoice::Exact(v(3)));
        assert_eq!(plan(b"x", &exact).unwrap().version, v(3));
        // AtLeast still grows past the minimum when the data needs it.
        let grows = base.with_version(VersionChoice::AtLeast(v(2)));
        assert_eq!(plan(&[b'x'; 100], &grows).unwrap().version, v(6));
    }

    #[test]
    fn exact_version_capacity_error_reports_that_version() {
        let o = EncodeOptions::default().with_version(VersionChoice::Exact(v(1)));
        let err = plan(&[b'x'; 15], &o).unwrap_err();
        assert_eq!(
            err,
            EncodeError::DataTooLong(CapacityError {
                needed: 15,
                max: 14,
                unit: CapacityUnit::Characters,
                mode: Mode::Byte,
                ec_level: EcLevel::M,
                version: v(1),
                bytes: 15,
            })
        );
    }

    #[test]
    fn from_parts_rejects_a_backend_that_ignored_the_plan() {
        let plan = EncodePlan {
            segments: vec![],
            version: v(1),
            ec_level: EcLevel::M,
            mask: Some(Mask::new(1).unwrap()),
        };
        let small = Matrix::from_modules(2, vec![false; 4]).unwrap();
        assert!(matches!(
            QrCode::from_parts(small, &plan, Mask::new(1).unwrap()),
            Err(EncodeError::Internal(_))
        ));
        let right_size = Matrix::from_modules(21, vec![false; 441]).unwrap();
        assert!(matches!(
            QrCode::from_parts(right_size.clone(), &plan, Mask::new(2).unwrap()),
            Err(EncodeError::Internal(_))
        ));
        let code = QrCode::from_parts(right_size, &plan, Mask::new(1).unwrap()).unwrap();
        assert_eq!(code.into_matrix().width(), 21);
    }

    #[test]
    fn encode_reports_the_parameters_used() {
        let o = EncodeOptions::default()
            .with_ec_level(EcLevel::Q)
            .with_mask(Mask::new(5));
        let code = encode(b"HELLO", &o).unwrap();
        assert_eq!(code.ec_level(), EcLevel::Q);
        assert_eq!(code.mode(), Mode::Alphanumeric);
        assert_eq!(code.mask(), Mask::new(5).unwrap());
        assert_eq!(code.version(), v(1));
        assert_eq!(code.matrix().width(), 21);
    }

    /// Mixed data is segmented: `qrcode`'s sizing fits this in version 1
    /// (single-mode byte encoding needed version 2). See
    /// `tests/differential.rs` for the full table.
    #[test]
    fn mixed_data_is_segmented() {
        let code = encode(b"tel:+441234567890", &EncodeOptions::default()).unwrap();
        assert_eq!((code.mode(), code.version()), (Mode::Byte, v(1)));
        assert!(code.is_mixed_mode());
        let modes: Vec<Mode> = code.segments().iter().map(|s| s.mode).collect();
        assert_eq!(modes, [Mode::Byte, Mode::Numeric]);

        let single = encode(b"HELLO", &EncodeOptions::default()).unwrap();
        assert!(!single.is_mixed_mode());
        assert_eq!(single.segments().len(), 1);
    }

    #[test]
    fn mixed_data_too_long_is_reported_exactly_in_bits() {
        // 4 bytes, then 5,000 digits: a byte segment and a numeric one.
        let mut data = b"ref:".to_vec();
        data.extend((0..5_000).map(|i| b"0123456789"[i % 10]));
        let at_v40_l = plan(&data, &EncodeOptions::default().with_ec_level(EcLevel::L)).unwrap();
        assert_eq!(at_v40_l.segments.len(), 2);
        let bits = at_v40_l.bits();

        // Pin the version so the same segments no longer fit.
        let exact = |n| EncodeOptions::default().with_version(VersionChoice::Exact(v(n)));
        let err = plan(&data, &exact(30).with_ec_level(EcLevel::L)).unwrap_err();
        let EncodeError::DataTooLong(e) = err else {
            panic!("expected a capacity error, got {err:?}")
        };
        assert_eq!(e.unit, CapacityUnit::Bits);
        assert_eq!((e.needed, e.max), (bits, data_bits(v(30), EcLevel::L)));
        assert_eq!((e.mode, e.bytes, e.version), (Mode::Byte, 5_004, v(30)));
        assert!(e.needed > e.max);
    }

    #[test]
    fn single_segment_auto_data_too_long_is_reported_in_characters() {
        let o = EncodeOptions::default().with_version(VersionChoice::Exact(v(1)));
        let EncodeError::DataTooLong(e) = plan(&[b'7'; 35], &o).unwrap_err() else {
            panic!("expected a capacity error")
        };
        assert_eq!(
            (e.unit, e.mode, e.needed, e.max),
            (CapacityUnit::Characters, Mode::Numeric, 35, 34)
        );
    }

    /// Review of PR 8b (GitHub #9): greedy segmentation needs 109 bits
    /// here (v2 at Q), one alphanumeric segment 101 (v1).
    #[test]
    fn auto_never_loses_to_one_segment_in_the_narrowest_mode() {
        let data = b"XSL790106502KNC+";
        let runs = segment::runs(data);
        let greedy = segment::optimize(&runs, v(1));
        assert_eq!(segment::total_bits(&greedy, v(1)), 109);
        let q = EncodeOptions::default().with_ec_level(EcLevel::Q);
        let p = plan(data, &q).unwrap();
        assert_eq!(p.version, v(1));
        assert_eq!(p.segments, single_segment(data));
        assert_eq!((p.mode(), p.bits()), (Mode::Alphanumeric, 101));
        let fixed = plan(data, &q.with_mode(ModeChoice::Fixed(Mode::Alphanumeric))).unwrap();
        assert_eq!(fixed.version, p.version);
    }

    /// The same pattern at full alphanumeric capacity (4,296 characters at
    /// v40-L) fits as one segment; greedy segments would need 27,933 bits.
    /// One more character is reported in characters, like a fixed mode.
    #[test]
    fn auto_keeps_the_single_mode_capacity_ceiling() {
        let pattern = b"XSL790106502KNC+";
        for ec in EcLevel::ALL {
            let n = max_chars(Mode::Alphanumeric, Version::MAX, ec);
            let data: Vec<u8> = pattern.iter().copied().cycle().take(n).collect();
            let o = EncodeOptions::default().with_ec_level(ec);
            let p = plan(&data, &o).unwrap();
            assert_eq!(
                (p.version, p.mode()),
                (Version::MAX, Mode::Alphanumeric),
                "{ec}"
            );
            assert_eq!(p.segments.len(), 1, "{ec}");

            let longer: Vec<u8> = pattern.iter().copied().cycle().take(n + 1).collect();
            let EncodeError::DataTooLong(e) = plan(&longer, &o).unwrap_err() else {
                panic!("expected a capacity error at {ec}")
            };
            assert_eq!(
                (e.unit, e.mode, e.needed, e.max),
                (CapacityUnit::Characters, Mode::Alphanumeric, n + 1, n),
                "{ec}"
            );
        }
        let n = max_chars(Mode::Alphanumeric, Version::MAX, EcLevel::L);
        assert_eq!(n, 4_296);
        let data: Vec<u8> = pattern.iter().copied().cycle().take(n).collect();
        let greedy = segment::optimize(&segment::runs(&data), Version::MAX);
        assert_eq!(segment::total_bits(&greedy, Version::MAX), 27_933);
    }

    #[test]
    fn plan_bits_counts_every_segment_header() {
        let p = plan(b"tel:+441234567890", &EncodeOptions::default()).unwrap();
        // Byte "tel:+" (4 + 8 + 40) and numeric 12 digits (4 + 10 + 40).
        assert_eq!(p.bits(), 52 + 54);
    }

    #[test]
    fn count_classes_change_at_versions_10_and_27() {
        assert_eq!(count_class(v(9)), 0);
        assert_eq!(count_class(v(10)), 1);
        assert_eq!(count_class(v(26)), 1);
        assert_eq!(count_class(v(27)), 2);
    }

    /// Pins `qrcode`'s lowest-penalty mask for empty data at version 40, so
    /// a change to mask selection is a deliberate diff.
    #[test]
    fn automatic_mask_for_empty_data_at_version_40_is_pinned() {
        let o = EncodeOptions::default().with_version(VersionChoice::Exact(v(40)));
        let code = encode(b"", &o).unwrap();
        assert_eq!(code.mask().number(), 4);
    }
}
