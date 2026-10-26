use std::fs::{read_to_string, write};
use std::path::PathBuf;

use tempfile::tempdir;
use tidymedia::run_cli;

#[test]
fn verify_exif_tsv_injection_flags_mismatch() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    let jpg = PathBuf::from(src_dir).join("IMG_20240101_120000.jpg");
    write(
        &jpg,
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00",
    )
    .unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("verify.json");
    let tsv_path = report_dir.path().join("exif.tsv");
    write(
        &tsv_path,
        format!(
            "{}\t2023:06:01 10:00:00\t-\t-\t-\t-\tPanasonic\tDMC-GF6\n",
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
        "verify with mismatched bucket should exit non-zero"
    );
    let json: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid");
    assert_eq!(json["compared"], 1);
    assert_eq!(json["mismatched"], 1);
    let entry = &json["entries"][0];
    assert_eq!(entry["actual_bucket"], "2024:01");
    assert_eq!(entry["exif_exp_bucket"], "2023:06");
    assert_eq!(entry["exif_from"], "DTO");
    assert_eq!(entry["exif_make"], "Panasonic");
    assert_eq!(entry["mismatch"], true);
}

#[test]
fn verify_exif_tsv_skips_malformed_timestamp_field() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    let jpg = PathBuf::from(src_dir).join("IMG_20240101_120000.jpg");
    write(
        &jpg,
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00",
    )
    .unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("verify.json");
    let tsv_path = report_dir.path().join("exif.tsv");
    write(
        &tsv_path,
        format!(
            "{}\t20230101120000\t2023:06:01 10:00:00\t-\t-\t-\tMake\tModel\n",
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
        "expected bucket 2023:06 conflicts with 2024:01"
    );
    let json: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid");
    let entry = &json["entries"][0];
    assert_eq!(entry["exif_from"], "QTCreationDate");
    assert_eq!(entry["exif_exp_bucket"], "2023:06");
}

#[test]
fn verify_exif_tsv_short_fields_yield_no_expected_bucket() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    let jpg = PathBuf::from(src_dir).join("IMG_20240101_120000.jpg");
    write(
        &jpg,
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00",
    )
    .unwrap();
    let out = tempdir().unwrap();
    let report_dir = tempdir().unwrap();
    let report_path = report_dir.path().join("verify.json");
    let tsv_path = report_dir.path().join("exif.tsv");
    write(
        &tsv_path,
        format!("{}\t-\t-\t-\t-\t-\tMake\tModel\n", jpg.display()),
    )
    .unwrap();
    run_cli([
        "tidymedia",
        "verify",
        "--output",
        out.path().to_str().unwrap(),
        src_dir,
        "--exif-tsv",
        tsv_path.to_str().unwrap(),
        "--report",
        report_path.to_str().unwrap(),
    ])
    .expect("verify with no expected bucket should succeed");
    let json: serde_json::Value =
        serde_json::from_str(&read_to_string(&report_path).unwrap()).expect("report json valid");
    assert_eq!(json["compared"], 1);
    assert_eq!(json["mismatched"], 0);
    let entry = &json["entries"][0];
    assert_eq!(entry["actual_bucket"], "2024:01");
    assert!(entry["exif_from"].is_null());
    assert!(entry["exif_exp_bucket"].is_null());
}

#[test]
fn verify_parses_special_filename_date_buckets() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    let jpg = b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00";
    write(PathBuf::from(src_dir).join("99-12-31.jpg"), jpg).unwrap();
    write(
        PathBuf::from(src_dir).join("IMG_6489(20211399-174530).jpg"),
        jpg,
    )
    .unwrap();
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
    assert_eq!(json["compared"], 2);
    let mut by_name = std::collections::HashMap::new();
    for entry in json["entries"].as_array().unwrap() {
        let name = entry["source_path"].as_str().unwrap();
        let base = std::path::Path::new(name)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        by_name.insert(base, entry);
    }
    assert_eq!(by_name["99-12-31.jpg"]["filename_bucket"], "1999:12");
    assert_eq!(
        by_name["IMG_6489(20211399-174530).jpg"]["filename_bucket"],
        "2021:01"
    );
}
