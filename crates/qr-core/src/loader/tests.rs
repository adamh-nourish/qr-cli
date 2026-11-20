// Generated test pixels: truncating casts are the point.
#![allow(clippy::cast_possible_truncation)]

use std::io;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder as _};

use super::*;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/{name}")).unwrap()
}

/// A `width` x `height` image in `layout`, encoded by `image`'s own encoders.
fn encode(kind: ImageKind, layout: ExtendedColorType, width: u32, height: u32) -> Vec<u8> {
    let channels = usize::from(layout.channel_count());
    let bytes_per_channel = if matches!(layout, ExtendedColorType::Rgba16 | ExtendedColorType::L16)
    {
        2
    } else {
        1
    };
    let len = (width * height) as usize * channels * bytes_per_channel;
    let data: Vec<u8> = (0..len).map(|i| (i * 37 % 251) as u8).collect();
    let mut out = Vec::new();
    match kind {
        ImageKind::Png => PngEncoder::new(&mut out)
            .write_image(&data, width, height, layout)
            .unwrap(),
        ImageKind::Jpeg => JpegEncoder::new(&mut out)
            .write_image(&data, width, height, layout)
            .unwrap(),
        ImageKind::Webp => WebPEncoder::new_lossless(&mut out)
            .write_image(&data, width, height, layout)
            .unwrap(),
    }
    out
}

/// The CRC-32 (ISO 3309) of `bytes`, as PNG chunks carry it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A small valid PNG whose header was then rewritten to declare `width` x
/// `height` (and, with `rgba16`, 8 bytes per pixel), with a correct CRC: a
/// decompression bomb for any decoder that trusts the header.
fn png_header(width: u32, height: u32, rgba16: bool) -> Vec<u8> {
    let layout = if rgba16 {
        ExtendedColorType::Rgba16
    } else {
        ExtendedColorType::Rgb8
    };
    let mut png = encode(ImageKind::Png, layout, 2, 2);
    // Signature (8), IHDR length and type (8), then width and height.
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}

/// A lossless WebP header declaring `side` x `side` pixels (at most
/// 16,384), followed by a few bytes of nonsense.
fn webp_header(side: u32) -> Vec<u8> {
    let dims = (side - 1) | ((side - 1) << 14);
    let mut chunk = vec![0x2f];
    chunk.extend_from_slice(&dims.to_le_bytes());
    chunk.extend_from_slice(&[0; 8]);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&u32::try_from(4 + 8 + chunk.len()).unwrap().to_le_bytes());
    out.extend_from_slice(b"WEBPVP8L");
    out.extend_from_slice(&u32::try_from(chunk.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&chunk);
    out
}

/// A small valid JPEG whose frame header was then rewritten to declare
/// `width` x `height`.
fn jpeg_header(width: u16, height: u16) -> Vec<u8> {
    let mut jpeg = encode(ImageKind::Jpeg, ExtendedColorType::Rgb8, 8, 8);
    let sof = jpeg.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    // Marker (2), length (2), precision (1), then height and width.
    jpeg[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
    jpeg[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
    jpeg
}

#[test]
fn sniffs_only_the_three_signatures() {
    assert_eq!(
        ImageKind::sniff(&png_header(1, 1, false)),
        Some(ImageKind::Png)
    );
    assert_eq!(
        ImageKind::sniff(&[0xff, 0xd8, 0xff, 0xe0]),
        Some(ImageKind::Jpeg)
    );
    assert_eq!(ImageKind::sniff(&webp_header(4)), Some(ImageKind::Webp));
    for wrong in [
        &b"GIF89a\x01\x00\x01\x00"[..],
        b"BM\x3a\x00\x00\x00",
        b"<svg xmlns='http://www.w3.org/2000/svg'/>",
        b"RIFF\x24\x00\x00\x00WAVEfmt ",
        b"\x89PNX\r\n\x1a\n",
        b"\xff\xd8\x00",
        b"%PDF-1.4",
    ] {
        assert_eq!(ImageKind::sniff(wrong), None, "{wrong:?}");
        assert!(matches!(
            decode_image(wrong),
            Err(LoadError::UnsupportedFormat)
        ));
    }
    assert_eq!(ImageKind::ALL.map(ImageKind::name), ["png", "jpeg", "webp"]);
    assert_eq!(ImageKind::Webp.to_string(), "webp");
}

#[test]
fn a_file_cut_inside_its_signature_is_truncated() {
    for (bytes, kind) in [
        (&b"\x89PN"[..], ImageKind::Png),
        (b"\xff\xd8", ImageKind::Jpeg),
        (b"RIFF\x10\x00\x00\x00WE", ImageKind::Webp),
        (b"RIFF", ImageKind::Webp),
    ] {
        let err = decode_image(bytes).unwrap_err();
        assert!(
            matches!(err, LoadError::Truncated { kind: k, bytes: n } if k == kind && n == bytes.len()),
            "{err:?}"
        );
    }
    assert_eq!(
        decode_image(b"\x89P").unwrap_err().to_string(),
        "the png image is truncated after 2 bytes"
    );
    assert!(matches!(decode_image(b""), Err(LoadError::Empty)));
}

#[test]
fn every_png_layout_decodes_to_rgba() {
    for layout in [
        ExtendedColorType::L8,
        ExtendedColorType::La8,
        ExtendedColorType::Rgb8,
        ExtendedColorType::Rgba8,
        ExtendedColorType::L16,
        ExtendedColorType::Rgba16,
    ] {
        let bitmap = decode_image(&encode(ImageKind::Png, layout, 7, 5)).unwrap();
        assert_eq!((bitmap.width(), bitmap.height()), (7, 5), "{layout:?}");
        assert_eq!(bitmap.pixels().len(), 7 * 5 * 4);
    }
    // Grey stays grey and opaque.
    let grey = decode_image(&encode(ImageKind::Png, ExtendedColorType::L8, 2, 1)).unwrap();
    assert_eq!(grey.pixels(), [0, 0, 0, 255, 37, 37, 37, 255]);
    assert!(grey.is_opaque());
    let rgba = decode_image(&encode(ImageKind::Png, ExtendedColorType::Rgba8, 2, 1)).unwrap();
    assert_eq!(rgba.pixels(), [0, 37, 74, 111, 148, 185, 222, 8]);
    assert!(!rgba.is_opaque());
}

#[test]
fn our_own_outputs_decode() {
    use crate::{EncodeOptions, RasterFormat, RenderOptions, encode as qr, render_raster};
    let code = qr(b"loader", &EncodeOptions::default()).unwrap();
    for format in RasterFormat::ALL {
        for transparent in [false, true] {
            let options = RenderOptions::default().with_transparent(transparent);
            let bytes = render_raster(code.matrix(), format, &options).unwrap();
            let bitmap = decode_image(&bytes).unwrap();
            assert_eq!(bitmap.width(), 290, "{format}");
            // The top-left corner is quiet zone: white, or clear.
            let alpha = bitmap.pixels()[3];
            assert_eq!(alpha == 0, transparent && format.has_alpha(), "{format}");
        }
    }
}

#[test]
fn jpeg_and_webp_layouts_decode() {
    for layout in [ExtendedColorType::L8, ExtendedColorType::Rgb8] {
        let bitmap = decode_image(&encode(ImageKind::Jpeg, layout, 9, 4)).unwrap();
        assert_eq!((bitmap.width(), bitmap.height()), (9, 4));
        assert!(bitmap.is_opaque());
    }
    for layout in [ExtendedColorType::Rgb8, ExtendedColorType::Rgba8] {
        let bitmap = decode_image(&encode(ImageKind::Webp, layout, 3, 6)).unwrap();
        assert_eq!((bitmap.width(), bitmap.height()), (3, 6));
    }
    // Lossy WebP with an alpha channel, progressive JPEG and CMYK JPEG.
    let lossy = decode_image(&fixture("lossy-alpha.webp")).unwrap();
    assert_eq!((lossy.width(), lossy.height()), (24, 16));
    assert!(!lossy.is_opaque(), "the corners are transparent");
    for name in ["progressive.jpg", "cmyk.jpg"] {
        let bitmap = decode_image(&fixture(name)).unwrap();
        assert_eq!((bitmap.width(), bitmap.height()), (24, 16), "{name}");
        // A red-to-blue gradient: red at the top.
        let [r, _, b, a] = bitmap.pixels()[..4] else {
            unreachable!()
        };
        assert!(r > 200 && b < 80 && a == 255, "{name}: {r} {b}");
    }
}

#[test]
fn huge_dimensions_are_refused_before_decoding() {
    // Each would need gigabytes if the decoder trusted the header.
    for (bytes, what) in [
        (png_header(100_000, 100_000, false), "png 100000^2"),
        (png_header(8_193, 1, false), "png just over the side limit"),
        (png_header(1, 8_193, false), "png just over, tall"),
        (webp_header(9_000), "webp 9000^2"),
        (webp_header(8_193), "webp just over"),
        (jpeg_header(60_000, 60_000), "jpeg 60000^2"),
        (jpeg_header(8_193, 8), "jpeg just over"),
    ] {
        let err = decode_image(&bytes).unwrap_err();
        assert!(
            matches!(
                err,
                LoadError::DimensionsTooLarge {
                    max: MAX_IMAGE_SIDE,
                    ..
                }
            ),
            "{what}: {err:?}"
        );
        assert!(
            err.to_string().contains("8192x8192 pixel limit"),
            "{what}: {err}"
        );
    }
}

#[test]
fn headers_that_need_too_much_memory_are_refused() {
    // 8,000 x 8,000 at 8 bytes per pixel is 512 MB.
    let err = decode_image(&png_header(8_000, 8_000, true)).unwrap_err();
    assert!(matches!(err, LoadError::TooMuchMemory { .. }), "{err:?}");
    assert!(err.to_string().contains("128 MiB"), "{err}");
    // 6,000 x 6,000 RGB is 108 MB, under the limit, but 144 MB as RGBA.
    let err = decode_image(&png_header(6_000, 6_000, false)).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::TooMuchMemory {
                bytes: Some(144_000_000),
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "decoding the image needs 144000000 bytes, over the 134217728-byte (128 MiB) limit"
    );
}

#[test]
fn truncated_and_corrupt_files_fail_to_decode() {
    for kind in ImageKind::ALL {
        let whole = encode(kind, ExtendedColorType::Rgb8, 40, 40);
        for cut in [20, whole.len() / 2, whole.len() * 3 / 4] {
            let err = decode_image(&whole[..cut]).unwrap_err();
            assert!(
                matches!(err, LoadError::Decode { kind: k, .. } if k == kind),
                "{kind} cut at {cut}: {err:?}"
            );
            assert!(err.is_invalid_image());
            let text = err.to_string();
            assert!(
                text.starts_with(&format!("cannot decode the {kind} image: ")),
                "{text}"
            );
        }
    }
    // A header whose image data is too short for it.
    let err = decode_image(&png_header(100, 100, false)).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::Decode {
                kind: ImageKind::Png,
                ..
            }
        ),
        "{err:?}"
    );
    // A JPEG decoder fills in a scan cut short, so the missing end
    // marker is what catches it.
    let jpeg = encode(ImageKind::Jpeg, ExtendedColorType::Rgb8, 40, 40);
    for cut in [jpeg.len() - 2, jpeg.len() * 3 / 4] {
        let err = decode_image(&jpeg[..cut]).unwrap_err();
        assert!(err.to_string().contains("truncated"), "{err}");
    }
    assert!(jpeg_is_complete(&jpeg));
    assert!(!jpeg_is_complete(b"\xff\xd8\xff\xd9"), "no scan");
}

/// A reader that fails the test if it is read at all.
struct Untouchable;

impl Read for Untouchable {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("the reader was read although the declared size is over the limit");
    }
}

/// A reader that fails.
struct Broken;

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "nope"))
    }
}

#[test]
fn oversize_files_are_refused_before_and_while_reading() {
    // Declared too large: refused before reading.
    let err = load_image(Untouchable, Some(MAX_IMAGE_FILE_BYTES + 1)).unwrap_err();
    assert!(
        matches!(err, LoadError::FileTooLarge { bytes: Some(bytes), max } if bytes == MAX_IMAGE_FILE_BYTES + 1 && max == MAX_IMAGE_FILE_BYTES),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "the image is 10485761 bytes, over the 10485760-byte (10 MiB) limit"
    );
    // Declared small (or not at all) but longer: the read stops one byte
    // past the limit.
    for declared in [None, Some(10)] {
        let endless = io::repeat(0).take(MAX_IMAGE_FILE_BYTES * 3);
        let err = load_image(endless, declared).unwrap_err();
        assert!(
            matches!(err, LoadError::FileTooLarge { bytes: None, .. }),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "the image is more than 10485760 bytes, over the 10485760-byte (10 MiB) limit"
        );
    }
    // Bytes in memory get the same limit.
    let big = vec![0; usize::try_from(MAX_IMAGE_FILE_BYTES).unwrap() + 1];
    assert!(matches!(
        decode_image(&big),
        Err(LoadError::FileTooLarge { .. })
    ));
    // Exactly at the limit is read (and then judged on its contents).
    let at_limit = io::repeat(0).take(MAX_IMAGE_FILE_BYTES);
    assert!(matches!(
        load_image(at_limit, Some(MAX_IMAGE_FILE_BYTES)),
        Err(LoadError::UnsupportedFormat)
    ));
}

#[test]
fn reads_from_a_reader() {
    let png = encode(ImageKind::Png, ExtendedColorType::Rgba8, 3, 3);
    let len = u64::try_from(png.len()).unwrap();
    let bitmap = load_image(png.as_slice(), Some(len)).unwrap();
    assert_eq!((bitmap.width(), bitmap.height()), (3, 3));
    assert_eq!(load_image(png.as_slice(), None).unwrap(), bitmap);

    let err = load_image(Broken, None).unwrap_err();
    assert!(matches!(&err, LoadError::Read(e) if e.kind() == io::ErrorKind::PermissionDenied));
    assert!(!err.is_invalid_image());
    assert_eq!(err.to_string(), "cannot read the image: nope");
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
fn bitmaps_check_their_size() {
    assert!(Bitmap::from_rgba(0, 1, Vec::new()).is_none());
    assert!(Bitmap::from_rgba(MAX_IMAGE_SIDE + 1, 1, vec![0; 4 * 8_193]).is_none());
    assert!(Bitmap::from_rgba(2, 2, vec![0; 15]).is_none());
    let bitmap = Bitmap::from_rgba(1, 2, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
    assert!(bitmap.is_opaque());
    assert_eq!(format!("{bitmap:?}"), "Bitmap { width: 1, height: 2, .. }");
    assert_eq!(rgba_len(u32::MAX, u32::MAX), None);
}

#[test]
fn messages_name_the_limit_and_the_values() {
    let err = LoadError::DimensionsTooLarge {
        dimensions: Some((9_000, 10)),
        max: MAX_IMAGE_SIDE,
    };
    assert_eq!(
        err.to_string(),
        "the image is 9000x10 pixels and exceeds the 8192x8192 pixel limit"
    );
    let err = LoadError::DimensionsTooLarge {
        dimensions: None,
        max: MAX_IMAGE_SIDE,
    };
    assert_eq!(
        err.to_string(),
        "the image exceeds the 8192x8192 pixel limit"
    );
    let err = LoadError::TooMuchMemory {
        bytes: None,
        max: MAX_IMAGE_ALLOC,
    };
    assert_eq!(
        err.to_string(),
        "decoding the image needs more memory, over the 134217728-byte (128 MiB) limit"
    );
    assert_eq!(
        LoadError::UnsupportedFormat.to_string(),
        "not a png, jpeg or webp image (the header matches none of them)"
    );
    assert_eq!(LoadError::Empty.to_string(), "the image file is empty");
    assert_eq!(lowercase_first(""), "");
    assert_eq!(lowercase_first("Format error"), "format error");
}
