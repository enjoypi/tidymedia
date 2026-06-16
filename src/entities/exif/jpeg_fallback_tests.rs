use super::parse_jpeg_app1_exif;

// ---------- 字节构造 helper ----------

/// 构造 APP1 Exif segment payload（含 6 字节 magic + 完整 TIFF header）。
fn exif_app1_payload(tiff: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + tiff.len());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(tiff);
    out
}

/// 完整 JPEG segment 帧 = `FF marker + len(BE u16) + payload`；
/// `len` 字段包含自身 2 字节但不含 marker。
fn jpeg_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(payload.len() + 2).unwrap();
    let mut out = Vec::with_capacity(4 + payload.len());
    out.push(0xFF);
    out.push(marker);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// 最小 TIFF header + IFD0 含 Make="Cam\0\0"。
fn minimal_tiff() -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&0x002A_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u16.to_le_bytes()); // IFD0 count
    // Make entry @ offset 10..22；data offset = 8+2+12+4 = 26
    tiff.extend_from_slice(&0x010f_u16.to_le_bytes());
    tiff.extend_from_slice(&2_u16.to_le_bytes());
    tiff.extend_from_slice(&5_u32.to_le_bytes());
    tiff.extend_from_slice(&26_u32.to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    tiff.extend_from_slice(b"Cam\0\0");
    tiff
}

fn jpeg_with_exif_app1() -> Vec<u8> {
    let mut buf = vec![0xFF, 0xD8]; // SOI
    buf.extend_from_slice(&jpeg_segment(0xE1, &exif_app1_payload(&minimal_tiff())));
    buf
}

// ---------- 正常路径 ----------

#[test]
fn parse_jpeg_reads_minimal_exif_app1() {
    let buf = jpeg_with_exif_app1();
    let ifd = parse_jpeg_app1_exif(&buf).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Cam"));
}

#[test]
fn parse_jpeg_finds_exif_app1_after_xmp_app1() {
    // 多 APP1 段：先 XMP（不含 Exif magic）后 Exif。
    let mut buf = vec![0xFF, 0xD8];
    let xmp_payload = b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>";
    buf.extend_from_slice(&jpeg_segment(0xE1, xmp_payload));
    buf.extend_from_slice(&jpeg_segment(0xE1, &exif_app1_payload(&minimal_tiff())));
    let ifd = parse_jpeg_app1_exif(&buf).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Cam"));
}

#[test]
fn parse_jpeg_skips_non_app1_segments() {
    // SOI + APP0 (JFIF) + APP1 Exif
    let mut buf = vec![0xFF, 0xD8];
    let jfif_payload = b"JFIF\0\x01\x02\x00\x00\x01\x00\x01\x00\x00";
    buf.extend_from_slice(&jpeg_segment(0xE0, jfif_payload));
    buf.extend_from_slice(&jpeg_segment(0xE1, &exif_app1_payload(&minimal_tiff())));
    let ifd = parse_jpeg_app1_exif(&buf).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Cam"));
}

#[test]
fn parse_jpeg_handles_fill_byte_between_markers() {
    // 在 SOI 后插入 FF fill byte（合法），再进入 APP1。
    let mut buf = vec![0xFF, 0xD8, 0xFF, 0xFF];
    buf.extend_from_slice(&jpeg_segment(0xE1, &exif_app1_payload(&minimal_tiff()))[1..]);
    let ifd = parse_jpeg_app1_exif(&buf).unwrap();
    assert_eq!(ifd.make.as_deref(), Some("Cam"));
}

// ---------- 拒绝路径 ----------

#[test]
fn parse_jpeg_rejects_non_jpeg() {
    assert!(parse_jpeg_app1_exif(b"NOT-A-JPEG").is_none());
}

#[test]
fn parse_jpeg_rejects_truncated_at_soi() {
    assert!(parse_jpeg_app1_exif(&[0xFF]).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_sos_before_app1() {
    // SOI + SOS（直接跳到压缩数据）→ 没机会扫到 APP1
    let buf = vec![0xFF, 0xD8, 0xFF, 0xDA];
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_eoi_before_app1() {
    let buf = vec![0xFF, 0xD8, 0xFF, 0xD9];
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_app1_lacks_exif_magic() {
    // APP1 但 payload 非 Exif（例如 Adobe XMP）→ 单段就到末尾 → None
    let mut buf = vec![0xFF, 0xD8];
    buf.extend_from_slice(&jpeg_segment(0xE1, b"http://ns.adobe.com/xap/1.0/\0xx"));
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_app1_tiff_malformed() {
    let mut buf = vec![0xFF, 0xD8];
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(b"NOT-A-TIFF-HEADER");
    buf.extend_from_slice(&jpeg_segment(0xE1, &payload));
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_segment_length_below_min() {
    // len 字段声称 1（< 2）→ 拒绝
    let buf = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01];
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_segment_length_truncated() {
    // 只有 marker 没有 len 字段
    let buf = vec![0xFF, 0xD8, 0xFF, 0xE1];
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_segment_payload_truncated() {
    // len 字段声称 1000 但实际只给了 5 字节 payload
    let mut buf = vec![0xFF, 0xD8, 0xFF, 0xE1];
    buf.extend_from_slice(&1000_u16.to_be_bytes());
    buf.extend_from_slice(&[0u8; 5]);
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_byte_after_soi_not_ff() {
    // SOI 后第 3 字节不是 0xFF → 不可能是 marker
    let buf = vec![0xFF, 0xD8, 0x00, 0xE1, 0x00, 0x06];
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_marker_code_unreadable() {
    // SOI + FF 但无后续 marker code 字节
    let buf = vec![0xFF, 0xD8, 0xFF];
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_marker_budget_exhausted() {
    // 70 个 APP0 段填满 → 命中 MAX_MARKERS 上限
    let mut buf = vec![0xFF, 0xD8];
    for _ in 0..70 {
        buf.extend_from_slice(&jpeg_segment(0xE0, &[0u8; 4]));
    }
    buf.extend_from_slice(&jpeg_segment(0xE1, &exif_app1_payload(&minimal_tiff())));
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}

#[test]
fn parse_jpeg_returns_none_when_payload_lacks_six_bytes() {
    // APP1 但 payload < 6 字节（连 Exif magic 都不够）→ payload.get(..6) None
    let mut buf = vec![0xFF, 0xD8];
    buf.extend_from_slice(&jpeg_segment(0xE1, b"X"));
    assert!(parse_jpeg_app1_exif(&buf).is_none());
}
