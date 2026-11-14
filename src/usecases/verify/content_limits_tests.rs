use std::io::Cursor;
use std::sync::Arc;

use camino::Utf8PathBuf;
use image::{Rgb, RgbImage};

use super::tests_common::legacy_rotated_similar;
use super::{OutputLibrary, rotated_phash_similar, verdict_for};
use crate::adapters::backend::fake::FakeBackend;
use crate::entities::file_info::Info;
use crate::entities::memory::{Budget, Level};
use crate::entities::uri::Location;

const WORK_FITS_64_NOT_128: u64 = 2_000_000;

fn local(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

fn noise(width: u32, height: u32, seed: u64) -> RgbImage {
    let mut state = seed;
    RgbImage::from_fn(width, height, |_, _| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let bytes = state.to_be_bytes();
        Rgb([bytes[0], bytes[1], bytes[2]])
    })
}

fn brighten(image: &RgbImage, delta: u8) -> RgbImage {
    let mut out = image.clone();
    for pixel in out.pixels_mut() {
        pixel.0 = pixel.0.map(|v| v.saturating_add(delta));
    }
    out
}

fn png(image: &RgbImage) -> Vec<u8> {
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(image.clone())
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("internal: png encode");
    buf
}

fn budget(level: Level, capacity: u64) -> &'static Budget {
    Box::leak(Box::new(Budget::new(level, capacity)))
}

fn scenario(src: &RgbImage, out: &RgbImage, hold: u64, work: u64) -> (Info, OutputLibrary) {
    let be = Arc::new(FakeBackend::new("local"));
    be.add_file(local("/src/a.png"), png(src));
    be.add_dir(local("/out"));
    be.add_file(local("/out/a.png"), png(out));
    let info = Info::open(&local("/src/a.png"), be.clone()).expect("internal: fake info open");
    let output: crate::usecases::Source = (local("/out"), be);
    let lib = OutputLibrary::from_source(&output)
        .with_budgets(budget(Level::Hold, hold), budget(Level::Work, work));
    (info, lib)
}

#[test]
fn source_beyond_work_budget_is_reported_too_large() {
    let (info, lib) = scenario(
        &noise(128, 128, 1),
        &noise(64, 64, 2),
        1 << 20,
        WORK_FITS_64_NOT_128,
    );
    assert_eq!(verdict_for(&info, &lib, 10, 1_000_000), "too_large");
}

#[test]
fn candidate_beyond_work_budget_is_reported_too_large() {
    let (info, lib) = scenario(
        &noise(64, 64, 3),
        &noise(128, 128, 4),
        1 << 20,
        WORK_FITS_64_NOT_128,
    );
    assert_eq!(verdict_for(&info, &lib, 10, 1_000_000), "too_large");
}

#[test]
fn candidate_beyond_size_gate_is_reported_too_large() {
    let source = noise(16, 16, 5);
    let (info, lib) = scenario(&source, &noise(128, 128, 6), 1 << 20, 1 << 30);
    assert_eq!(verdict_for(&info, &lib, 10, info.size), "too_large");
}

#[test]
fn source_beyond_size_gate_is_reported_too_large() {
    let (info, lib) = scenario(&noise(64, 64, 7), &noise(64, 64, 8), 1 << 20, 1 << 30);
    assert_eq!(verdict_for(&info, &lib, 10, info.size - 1), "too_large");
}

#[test]
fn bytes_beyond_hold_budget_are_reported_too_large() {
    let (info, lib) = scenario(&noise(64, 64, 9), &noise(64, 64, 10), 16, 1 << 30);
    assert_eq!(verdict_for(&info, &lib, 10, 1_000_000), "too_large");
}

#[test]
fn different_content_within_budgets_stays_name_only() {
    let (info, lib) = scenario(&noise(64, 64, 11), &noise(64, 64, 12), 1 << 20, 1 << 30);
    assert_eq!(verdict_for(&info, &lib, 10, 1_000_000), "name_only");
}

#[test]
fn unreadable_source_stays_name_only() {
    let be = Arc::new(FakeBackend::new("local"));
    be.add_file(local("/src/a.png"), png(&noise(16, 16, 13)));
    be.add_dir(local("/out"));
    be.add_file(local("/out/a.png"), png(&noise(16, 16, 14)));
    let info = Info::open(&local("/src/a.png"), be.clone()).expect("internal: fake info open");
    be.inject_reader_error(local("/src/a.png"), std::io::ErrorKind::TimedOut);
    let output: crate::usecases::Source = (local("/out"), be);
    let lib = OutputLibrary::from_source(&output);
    assert_eq!(verdict_for(&info, &lib, 10, 1_000_000), "name_only");
}

fn mapped_verdict(unchanged: bool) -> String {
    let image = noise(64, 64, 15);
    let be = Arc::new(FakeBackend::new("local"));
    be.add_file(local("/src/a.bmp"), png(&image));
    be.add_dir(local("/out"));
    be.add_file(local("/out/a.bmp"), png(&image::imageops::rotate90(&image)));
    let info = Info::open(&local("/src/a.bmp"), be.clone()).expect("internal: fake info open");
    be.set_mappable(unchanged);
    let output: crate::usecases::Source = (local("/out"), be);
    let lib = OutputLibrary::from_source(&output);
    verdict_for(&info, &lib, 10, 1_000_000)
}

#[test]
fn mapped_bytes_compare_like_heap_bytes_and_changes_downgrade_to_name_only() {
    assert_eq!(mapped_verdict(true), "rotated_same");
    assert_eq!(mapped_verdict(false), "name_only");
}

#[test]
fn rotation_verdict_matches_legacy_implementation() {
    let base = noise(160, 96, 21);
    let other = noise(160, 96, 22);
    let variants = [
        base.clone(),
        image::imageops::rotate90(&base),
        image::imageops::rotate180(&base),
        image::imageops::rotate270(&base),
        brighten(&base, 3),
        brighten(&base, 40),
        image::imageops::rotate90(&brighten(&base, 3)),
        other,
        noise(96, 96, 23),
    ];
    let encoded: Vec<Vec<u8>> = variants.iter().map(png).collect();
    let mut similar = 0;
    for (i, other) in encoded.iter().enumerate() {
        for (a, b) in [(&encoded[0], other), (other, &encoded[i / 2])] {
            let expected = legacy_rotated_similar(a, b, 20);
            assert_eq!(rotated_phash_similar(a, b, 20), expected);
            similar += usize::from(expected);
        }
    }
    assert!(similar > 0);
    assert!(!legacy_rotated_similar(b"not-an-image", &encoded[0], 10));
}
