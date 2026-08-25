// verify 内容比对（content_diff）e2e：像素/旋转/名称判定 + max_bytes 读 guard。
use std::fs::write;
use std::path::PathBuf;

use tempfile::tempdir;

use super::verify::{
    encode_png, minimal_jpeg, noise_image, verify_json, write_small_max_bytes_config,
};

fn oversized_jpeg(fill: u8) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF];
    b.extend(std::iter::repeat_n(fill, 1_200_000));
    b
}

#[test]
fn verify_pixel_same_when_metadata_differs_but_pixels_match() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    write(
        PathBuf::from(src_dir).join("photo.jpg"),
        minimal_jpeg(b"meta-a", b"PIXELSTREAM"),
    )
    .unwrap();
    let out = tempdir().unwrap();
    write(
        out.path().join("photo.jpg"),
        minimal_jpeg(b"meta-b", b"PIXELSTREAM"),
    )
    .unwrap();
    let report_dir = tempdir().unwrap();
    let json = verify_json(src_dir, out.path().to_str().unwrap(), &report_dir);
    assert_eq!(json["compared"], 1);
    assert_eq!(json["entries"][0]["duplicate_verdict"], "pixel_same");
}

#[test]
fn verify_rotated_same_when_candidate_is_rotated_copy() {
    let img = noise_image(128, 128, 41);
    let rotated = image::imageops::rotate90(&img);
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    write(PathBuf::from(src_dir).join("r.png"), encode_png(&img)).unwrap();
    let out = tempdir().unwrap();
    write(out.path().join("r.png"), encode_png(&rotated)).unwrap();
    let report_dir = tempdir().unwrap();
    let json = verify_json(src_dir, out.path().to_str().unwrap(), &report_dir);
    assert_eq!(json["compared"], 1);
    assert_eq!(json["entries"][0]["duplicate_verdict"], "rotated_same");
}

#[test]
fn verify_name_only_when_candidates_do_not_match_content() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    write(
        PathBuf::from(src_dir).join("IMG_20240101.jpg"),
        minimal_jpeg(b"s", b"SRCPIX"),
    )
    .unwrap();
    let out = tempdir().unwrap();
    write(
        out.path().join("IMG_20240101.jpg"),
        minimal_jpeg(b"c1", b"CAND1PX"),
    )
    .unwrap();
    write(
        out.path().join("IMG_20240101_2.jpg"),
        minimal_jpeg(b"c2", b"CAND2PX"),
    )
    .unwrap();
    write(
        out.path().join("IMG_20240101_abc.jpg"),
        minimal_jpeg(b"c3", b"CAND3PX"),
    )
    .unwrap();
    write(
        out.path().join("IMG_20240101_.jpg"),
        minimal_jpeg(b"c4", b"CAND4PX"),
    )
    .unwrap();
    write(
        out.path().join("IMG_20240101.png"),
        minimal_jpeg(b"c5", b"CAND5PX"),
    )
    .unwrap();
    write(
        out.path().join("other.jpg"),
        minimal_jpeg(b"c6", b"OTHERPX"),
    )
    .unwrap();
    let report_dir = tempdir().unwrap();
    let json = verify_json(src_dir, out.path().to_str().unwrap(), &report_dir);
    assert_eq!(json["compared"], 1);
    assert_eq!(json["entries"][0]["duplicate_verdict"], "name_only");
}

#[test]
fn verify_name_only_when_max_bytes_guard_blocks_reads() {
    let _cfg = write_small_max_bytes_config();
    let big_a = oversized_jpeg(0xAA);
    let big_b = oversized_jpeg(0xBB);
    let big_c = oversized_jpeg(0xCC);
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    write(PathBuf::from(src_dir).join("big.jpg"), big_a).unwrap();
    write(
        PathBuf::from(src_dir).join("small.jpg"),
        b"\xFF\xD8\xFF\x00",
    )
    .unwrap();
    let out = tempdir().unwrap();
    write(out.path().join("big.jpg"), big_b).unwrap();
    write(out.path().join("small.jpg"), big_c).unwrap();
    let report_dir = tempdir().unwrap();
    let json = verify_json(src_dir, out.path().to_str().unwrap(), &report_dir);
    assert_eq!(json["compared"], 2);
    // 源超 max_bytes → 读 guard 拦截 → name_only；候选超 max_bytes → 读 guard 拦截 → name_only
    assert_eq!(json["entries"][0]["duplicate_verdict"], "name_only");
    assert_eq!(json["entries"][1]["duplicate_verdict"], "name_only");
}

#[test]
fn verify_with_file_output_indexes_nothing() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    write(
        PathBuf::from(src_dir).join("photo.jpg"),
        minimal_jpeg(b"m", b"PIX"),
    )
    .unwrap();
    let out = tempdir().unwrap();
    let outfile = out.path().join("out.txt");
    write(&outfile, b"not a dir").unwrap();
    let report_dir = tempdir().unwrap();
    let json = verify_json(src_dir, outfile.to_str().unwrap(), &report_dir);
    assert_eq!(json["compared"], 1);
    assert_eq!(json["entries"][0]["duplicate_verdict"], "absent");
}

#[test]
fn verify_name_only_for_extensionless_source_with_unavailable_entropy() {
    let src = tempdir().unwrap();
    let src_dir = src.path().to_str().unwrap();
    // 无扩展名源（split_stem_ext None 分支）+ 4 字节 JPEG magic（entropy 不可用）
    write(PathBuf::from(src_dir).join("oddphoto"), b"\xFF\xD8\xFF\x00").unwrap();
    let out = tempdir().unwrap();
    write(out.path().join("oddphoto"), minimal_jpeg(b"c", b"ODDPX")).unwrap();
    write(out.path().join("oddphoto_2"), minimal_jpeg(b"d", b"ODDPX2")).unwrap();
    let report_dir = tempdir().unwrap();
    let json = verify_json(src_dir, out.path().to_str().unwrap(), &report_dir);
    assert_eq!(json["compared"], 1);
    assert_eq!(json["entries"][0]["duplicate_verdict"], "name_only");
}
