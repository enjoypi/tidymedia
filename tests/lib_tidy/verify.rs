//! `verify` 子命令 e2e：tidy 调度 + `run_cli` 字符串形式 + report JSON 断言。

use std::fs;
use std::fs::{read_to_string, write};
use std::io::Cursor;
use std::path::PathBuf;

use image::RgbImage;
use tempfile::tempdir;
use tidymedia::{Commands, reset_config_loader, run_cli, tidy};

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
    // include_non_media=true 反向：非媒体也进对账（`!include_non_media` 短路）。
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
    // mtime 1904-01-01（pre-epoch）→ fs_time::from_modified 返 None → 无 P4 候选；
    // 文件名无日期 + 无 EXIF → media_time_decision 为 None → decision_failed 计入，
    // dispatch 部分失败 arm 让 `$?` 非 0。
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
    // DTO 是 14 位无冒号时间（长度≥7 但第 5 字节非 `:`）→ expected_bucket 跳过该
    // 字段，落到下一个合法字段 QTCreationDate（QT 列走 UTC→tz 转换）。
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
    // 全部字段是 `-`（1 字符 < 7）→ expected_bucket 的 `v.len() >= 7` 短路逐列
    // 跳过 → (None, None)，exif_from / exif_exp_bucket 均空，无 mismatch。
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
    // YY≥50 → 1900+YY：`99-12-31` → 1999:12。
    assert_eq!(by_name["99-12-31.jpg"]["filename_bucket"], "1999:12");
    // 括号紧凑时戳形状合法但日期非法（2021-13-99）→ 该 matcher 跳过不产候选。
    assert_eq!(
        by_name["IMG_6489(20211399-174530).jpg"]["filename_bucket"],
        "2021:01"
    );
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
    // sanitize 下限 1 MiB：取值 1048576，测试文件 1.2 MB > 1 MiB 触发读 guard
    fs::write(
        &cfg_path,
        "backend:\n  face:\n    max_image_bytes: 1048576\n",
    )
    .unwrap();
    // SAFETY: nextest 每测试独立进程，无并发 env 修改竞争
    unsafe {
        std::env::set_var("TIDYMEDIA_CONFIG", cfg_path.to_str().unwrap());
    }
    tidymedia::install_config_loader();
    dir
}
