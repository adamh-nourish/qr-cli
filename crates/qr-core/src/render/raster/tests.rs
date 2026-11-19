use super::*;

#[test]
fn names_round_trip() {
    for format in RasterFormat::ALL {
        assert_eq!(format.to_string().parse(), Ok(format));
        assert_eq!(format.extension().to_uppercase().parse(), Ok(format));
    }
    assert_eq!("JPG".parse(), Ok(RasterFormat::Jpeg));
    assert_eq!(RasterFormat::Png.media_type(), "image/png");
    assert_eq!(RasterFormat::Jpeg.media_type(), "image/jpeg");
    assert_eq!(RasterFormat::Webp.media_type(), "image/webp");
    let err = "svg".parse::<RasterFormat>().unwrap_err();
    assert_eq!(
        err.to_string(),
        "unknown raster format 'svg' (expected png, jpeg or webp)"
    );
}

#[test]
fn capabilities() {
    use RasterFormat as F;
    assert!(F::Png.records_dpi() && F::Jpeg.records_dpi() && !F::Webp.records_dpi());
    assert!(F::Png.has_alpha() && !F::Jpeg.has_alpha() && F::Webp.has_alpha());
}

#[test]
fn dispatches_to_each_renderer() {
    let m = Matrix::from_modules(1, vec![true]).unwrap();
    let o = RenderOptions::default();
    assert_eq!(render_raster(&m, RasterFormat::Png, &o), render_png(&m, &o));
    assert_eq!(
        render_raster(&m, RasterFormat::Jpeg, &o),
        render_jpeg(&m, &o)
    );
    assert_eq!(
        render_raster(&m, RasterFormat::Webp, &o),
        render_webp(&m, &o)
    );
    let zero = o.with_scale(0);
    for format in RasterFormat::ALL {
        assert_eq!(
            render_raster(&m, format, &zero),
            Err(RenderError::ZeroScale)
        );
    }
}
