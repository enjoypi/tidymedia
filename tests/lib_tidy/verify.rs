use std::fs;
use std::fs::{read_to_string, write};
use std::io::Cursor;
use std::path::PathBuf;

use image::RgbImage;
use tempfile::tempdir;
use tidymedia::{
    Commands, DefaultDetectorFactory, Location, reset_config_loader, run_cli, tidy, tidy_with,
};

use super::{DATA_DIR, FakeBackendFactory, local, smb_loc};

#[test]
fn tidy_dispatches_verify_on_data_dir() {
    let out = tempdir().unwrap();
    tidy(Commands::Verify {
        sources: vec![local(DATA_DIR)],
        output: local(out.path().to_str().unwrap()),
        include_non_media: false,
        exif_tsv: None,
        phash_max: None,
        report: None,
    })
    .expect("verify should succeed on data dir");
}

#[test]
fn verify_writes_json_report_with_scanned_and_entries() {
    let out = tempdir().unwrap();
    let out_dir = out.path().to_str().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("verify.json");
    run_cli([
        "tidymedia",
        "verify",
        "--output",
        out_dir,
        DATA_DIR,
        "--report",
        report_path.to_str().unwrap(),
    ])
    .expect("verify via run_cli should succeed");
    let json: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid");
    let scanned = json["scanned"].as_u64().expect("scanned field");
    let compared = json["compared"].as_u64().expect("compared field");
    assert!(scanned > 0);
    assert!(compared <= scanned, "compared must not exceed scanned");
    assert_eq!(json["dry_run"], true);
    let compared_usize = usize::try_from(compared).expect("compared fits usize");
    assert_eq!(
        json["entries"].as_array().map(Vec::len),
        Some(compared_usize),
        "entries count must equal compared (media files)"
    );
}

#[test]
fn verify_marks_exact_dup_when_output_has_identical_copy() {
    let bytes: &[u8] = b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00";
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    write(
        PathBuf::from(src_dir).join("IMG_20240101_120000.jpg"),
        bytes,
    )
    .unwrap();
    let out = tempdir().unwrap();
    write(out.path().join("IMG_20240101_120000.jpg"), bytes).unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("verify.json");
    run_cli([
        "tidymedia",
        "verify",
        "--output",
        out.path().to_str().unwrap(),
        src_dir,
        "--report",
        report_path.to_str().unwrap(),
    ])
    .expect("verify should succeed");
    let json: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid");
    assert_eq!(json["compared"], 1);
    assert_eq!(json["entries"][0]["duplicate_verdict"], "exact_dup");
}

#[test]
fn verify_skips_non_media_when_not_included() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    write(
        PathBuf::from(src_dir).join("IMG_20240101_120000.jpg"),
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00",
    )
    .unwrap();
    write(PathBuf::from(src_dir).join("notes.txt"), b"plain text").unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("verify.json");
    run_cli([
        "tidymedia",
        "verify",
        "--output",
        out.path().to_str().unwrap(),
        src_dir,
        "--report",
        report_path.to_str().unwrap(),
    ])
    .expect("verify should succeed");
    let json: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid");
    assert_eq!(json["scanned"], 2);
    assert_eq!(json["compared"], 1);
    let report_path2 = report_dir.path().join("verify-incl.json");
    run_cli([
        "tidymedia",
        "verify",
        "--include-non-media",
        "--output",
        out.path().to_str().unwrap(),
        src_dir,
        "--report",
        report_path2.to_str().unwrap(),
    ])
    .expect("verify with include-non-media should succeed");
    let json2: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path2).unwrap()).expect("report json valid");
    assert_eq!(json2["scanned"], 2);
    assert_eq!(json2["compared"], 2);
}

#[test]
fn verify_marks_unresolved_decision_and_exits_non_zero() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    let jpg = PathBuf::from(src_dir).join("no-date.jpg");
    write(
        &jpg,
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00",
    )
    .unwrap();
    let pre_epoch = filetime::FileTime::from_unix_time(-2_082_844_800, 0);
    filetime::set_file_mtime(&jpg, pre_epoch).expect("set pre-epoch mtime");
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("verify.json");
    let tsv_path = report_dir.path().join("exif.tsv");
    write(
        &tsv_path,
        format!(
            "{}\t0000:00:00 00:00:00\t-\t-\t-\t-\tMake\tModel\n",
            jpg.display()
        ),
    )
    .unwrap();
    let result = run_cli([
        "tidymedia",
        "verify",
        "--output",
        out.path().to_str().unwrap(),
        src_dir,
        "--exif-tsv",
        tsv_path.to_str().unwrap(),
        "--report",
        report_path.to_str().unwrap(),
    ]);
    assert!(
        result.is_err(),
        "verify with unresolved decision should exit non-zero"
    );
    let json: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid");
    assert_eq!(json["compared"], 1);
    assert_eq!(json["decision_failed"], 1);
    assert_eq!(json["mismatched"], 0);
    let entry = &json["entries"][0];
    assert_eq!(entry["exif_exp_bucket"], "0000:00");
    assert_eq!(entry["exif_from"], "DTO");
    assert_eq!(entry["patterns"][0], "CameraClockUnset");
    assert!(entry["fix_suggestion"].is_string());
}

pub(super) fn minimal_jpeg(meta: &[u8], pixels: &[u8]) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8];
    b.extend_from_slice(&[0xFF, 0xE0]);
    b.extend_from_slice(&(u16::try_from(meta.len()).unwrap() + 2).to_be_bytes());
    b.extend_from_slice(meta);
    b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0x01, 0x02]);
    b.extend_from_slice(pixels);
    b
}

pub(super) fn noise_image(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    for (i, px) in img.pixels_mut().enumerate() {
        let v = u8::try_from(((i as u64).wrapping_mul(seed) ^ (i as u64 >> 3)) & 0xff)
            .expect("internal: & 0xff < 256");
        px.0 = [v, v, v];
    }
    img
}

pub(super) fn encode_png(img: &RgbImage) -> Vec<u8> {
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(img.clone())
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("internal: png encode");
    buf
}

pub(super) fn verify_json(
    src_dir: &str,
    out_dir: &str,
    report_dir: &tempfile::TempDir,
) -> serde_json::Value {
    let report_path = report_dir.path().join("verify.json");
    run_cli([
        "tidymedia",
        "verify",
        "--output",
        out_dir,
        src_dir,
        "--report",
        report_path.to_str().unwrap(),
    ])
    .expect("verify should succeed");
    serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid")
}

pub(super) fn write_small_max_bytes_config() -> tempfile::TempDir {
    reset_config_loader();
    let dir = tempdir().unwrap();
    let cfg_path = dir.path().join("config.yaml");
    fs::write(&cfg_path, "verify:\n  max_image_bytes: 1048576\n").unwrap();
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }
    tidymedia::install_config_loader();
    dir
}

fn verify_command(sources: Vec<Location>, output: Location) -> Commands {
    Commands::Verify {
        sources,
        output,
        include_non_media: false,
        exif_tsv: None,
        phash_max: None,
        report: None,
    }
}

#[test]
fn tidy_verify_rejects_unbuildable_source() {
    let out = tempdir().unwrap();
    let res = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        verify_command(vec![smb_loc("")], local(out.path().to_str().unwrap())),
    );
    assert!(res.is_err());
}

#[test]
fn tidy_verify_rejects_unbuildable_output() {
    let res = tidy_with(
        &FakeBackendFactory::new(),
        &DefaultDetectorFactory,
        verify_command(vec![local(DATA_DIR)], smb_loc("")),
    );
    assert!(res.is_err());
}
