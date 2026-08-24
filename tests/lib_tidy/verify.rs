//! `verify` 子命令 e2e：tidy 调度 + `run_cli` 字符串形式 + report JSON 断言。

use std::fs::{read_to_string, write};
use std::path::PathBuf;

use tempfile::tempdir;
use tidymedia::{Commands, run_cli, tidy};

use super::{DATA_DIR, local};

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
    // exiftool 8 列契约：path/DTO/QT:CreationDate/QT:CreateDate/CreateDate/FileModifyDate/Make/Model。
    // DTO 给 2023:06（与文件名推导的 2024:01 桶冲突）→ 应报 mismatch。
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
    // 找到 MISMATCH → `tidy()` 部分失败 arm 让 `$?` 非 0（诊断必须可被 CI 感知）。
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
    // output 里放同内容同 basename 的已归档副本 → SHA-512 命中 exact_dup。
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
