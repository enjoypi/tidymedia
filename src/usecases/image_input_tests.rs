use super::*;
use crate::entities::memory::Level;

fn png() -> Vec<u8> {
    std::fs::read("tests/data/sample-png-exif.png").expect("internal: png fixture")
}

fn ctx(work: &Budget) -> DecodeCtx<'_> {
    DecodeCtx { work, factor: 3 }
}

#[test]
fn estimate_adds_conversion_only_for_non_rgb8() {
    let rgb = Header {
        width: 100,
        height: 50,
        total_bytes: 15_000,
        rgb8: true,
    };
    let gray = Header {
        total_bytes: 5_000,
        rgb8: false,
        ..rgb
    };
    let base = 1 << 20;
    assert_eq!(estimate(&rgb, 3, 512), base + 45_000 + 100 * 512 * 16);
    assert_eq!(
        estimate(&gray, 3, 512),
        base + 15_000 + 15_000 + 100 * 512 * 16
    );
    assert_eq!(
        estimate(
            &Header {
                total_bytes: u64::MAX,
                ..rgb
            },
            3,
            512
        ),
        u64::MAX
    );
}

#[test]
fn with_rgb_decodes_and_releases_budget() {
    let work = Budget::new(Level::Work, 1 << 30);
    let dims = with_rgb(&png(), ctx(&work), 512, RgbImage::dimensions).unwrap();
    assert!(dims.0 > 0 && dims.1 > 0);
    assert!(work.reserve(1 << 30).is_ok());
}

#[test]
fn with_rgb_reports_too_large_without_decoding() {
    let work = Budget::new(Level::Work, 16);
    let err = with_rgb(&png(), ctx(&work), 512, |_| ()).unwrap_err();
    let DecodeError::TooLarge(too_large) = &err else {
        panic!("expected TooLarge");
    };
    assert_eq!(too_large.capacity, 16);
    assert!(err.to_string().starts_with("image too large"));
}

#[test]
fn with_rgb_rejects_non_image_and_truncated_body() {
    let work = Budget::new(Level::Work, 1 << 30);
    let err = with_rgb(b"not-an-image", ctx(&work), 512, |_| ()).unwrap_err();
    assert!(matches!(err, DecodeError::Invalid(_)));
    assert!(err.to_string().starts_with("decode image"));
    let bytes = png();
    let truncated = &bytes[..bytes.len() - 20];
    assert!(matches!(
        with_rgb(truncated, ctx(&work), 512, |_| ()),
        Err(DecodeError::Invalid(_))
    ));
}

#[test]
fn global_ctx_uses_configured_factor() {
    let global = DecodeCtx::global();
    assert_eq!(
        global.factor,
        crate::usecases::config::config().memory.decode_factor
    );
    assert!(global.work.capacity() > 0);
}

#[test]
fn calibrate_returns_estimate_only_for_decodable_images() {
    let bytes = png();
    assert!(calibrate(&bytes).unwrap() > 0);
    assert_eq!(calibrate(b"not-an-image"), None);
    assert_eq!(calibrate(&bytes[..bytes.len() - 20]), None);
}
