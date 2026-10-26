use camino::Utf8Path;
use tidymedia::{OcrConfig, dbnet_with_error, dbnet_with_output};

fn cfg() -> OcrConfig {
    OcrConfig {
        det_model_path: "unused-by-stub".into(),
        binarize_threshold: 0.3,
        min_text_pixel_ratio: 0.005,
        resize_max_side: 736,
        max_image_bytes: 1024,
    }
}

fn tiny_png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&[255_u8, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
        .expect("encode tiny png");
    out
}

#[test]
fn dbnet_has_text_success() {
    let detector = dbnet_with_output(cfg(), 0.9);
    assert!(
        detector
            .has_text(Utf8Path::new("/x.png"), &tiny_png())
            .unwrap()
    );
}

#[test]
fn dbnet_rejects_invalid_image() {
    let detector = dbnet_with_output(cfg(), 0.9);
    let err = detector
        .has_text(Utf8Path::new("/bad.png"), b"not-an-image")
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn dbnet_propagates_raw_error() {
    let detector = dbnet_with_error(cfg());
    let err = detector
        .has_text(Utf8Path::new("/x.png"), &tiny_png())
        .unwrap_err();
    assert!(err.to_string().contains("stub tract failure"));
}

#[path = "support/tracing_fixture_tests.rs"]
mod tracing_fixture;
