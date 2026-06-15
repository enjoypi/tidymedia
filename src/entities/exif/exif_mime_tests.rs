use super::Exif;
use super::tests_common::utc;

// 老 QuickTime `pnot` preview atom 起头的 MOV 文件：infer crate 只认 `ftyp`，
// 必须靠 fallback 兜底返回 `video/quicktime`，否则 `is_media` 误判致整文件被 ignore。
#[test]
fn quicktime_legacy_mime_detects_pnot_atom() {
    let mut buf = vec![0u8, 0, 0, 0x14];
    buf.extend_from_slice(b"pnot");
    assert_eq!(super::quicktime_legacy_mime(&buf), Some("video/quicktime"));
}

// mdat-first MOV 变体（无任何头 atom、moov 在文件末尾的早期 QuickTime）：
// `infer` 0.19 不识别 (无 ftyp)；旧实现仅查 pnot 也漏识 → MIME 为空 →
// from_reader 不调 populate_video_dates → fork 后的 nom-exif 永远拿不到执行机会
// → is_media() false，整段视频被 ignore（CLAUDE.md「项目 Gotcha」mdat-first 条）。
#[test]
fn quicktime_legacy_mime_detects_mdat_atom() {
    let mut buf = vec![0u8, 0x10, 0, 0]; // mdat 大 box size
    buf.extend_from_slice(b"mdat");
    buf.extend_from_slice(&[0u8; 32]); // 后续 body 字节（数 MB-级，此处仅占位）
    assert_eq!(super::quicktime_legacy_mime(&buf), Some("video/quicktime"));
}

#[test]
fn quicktime_legacy_mime_unknown_tag_returns_none() {
    let mut buf = vec![0u8, 0, 0, 0x14];
    buf.extend_from_slice(b"XXXX");
    assert!(super::quicktime_legacy_mime(&buf).is_none());
}

#[test]
fn quicktime_legacy_mime_too_short_returns_none() {
    let buf = [0u8; 7];
    assert!(super::quicktime_legacy_mime(&buf).is_none());
}

// BDAV M2TS（AVCHD .mts/.m2ts）：4-byte TP_extra_header + 188-byte TS packet。
// `infer` 0.19 不识别；fallback 要求 offset 4 + 196 连续两个 0x47 sync byte。
#[test]
fn m2ts_legacy_mime_detects_bdav_sync_pair() {
    let mut buf = vec![0u8; 256];
    buf[4] = 0x47;
    buf[196] = 0x47;
    assert_eq!(super::m2ts_legacy_mime(&buf), Some("video/m2ts"));
}

// 单 sync byte 不够 —— 任意二进制都可能在某 offset 命中 0x47。
#[test]
fn m2ts_legacy_mime_single_sync_returns_none() {
    let mut buf = vec![0u8; 256];
    buf[4] = 0x47;
    assert!(super::m2ts_legacy_mime(&buf).is_none());
}

#[test]
fn m2ts_legacy_mime_too_short_returns_none() {
    let buf = [0u8; 100];
    assert!(super::m2ts_legacy_mime(&buf).is_none());
}

// End-to-end：FakeBackend 喂 BDAV pattern bytes → Exif::open 走 m2ts fallback，
// 让 is_media() 通过门槛，整段 AVCHD 视频不被 ignore（之前 28 个 .MTS 文件残留场景）。
#[test]
fn open_uses_m2ts_legacy_fallback_for_bdav_pattern() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let mut bytes = vec![0u8; 256];
    bytes[4] = 0x47;
    bytes[196] = 0x47;

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/clip.mts"));
    fake.add_file(loc.clone(), bytes);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open(&loc, &backend, utc()).unwrap();
    assert_eq!(exif.mime_type(), "video/m2ts");
    assert!(exif.is_media());
}

// 3GPP 手机视频（常伪装 `.mp4` 扩展名）：标准 BMFF `ftyp` 但 brand 是 `3gp4`/`3gp5`；
// `infer` 0.19 的 MP4 matcher 不认 `3gp*` brand，不识别会让整段 3GP 被 ignore。
#[test]
fn bmff_3gpp_mime_detects_3gp_brand() {
    let mut buf = vec![0u8, 0, 0, 0x1c];
    buf.extend_from_slice(b"ftyp3gp5");
    assert_eq!(super::bmff_3gpp_mime(&buf), Some("video/3gpp"));
}

#[test]
fn bmff_3gpp_mime_other_brand_returns_none() {
    let mut buf = vec![0u8, 0, 0, 0x1c];
    buf.extend_from_slice(b"ftypisom");
    assert!(super::bmff_3gpp_mime(&buf).is_none());
}

#[test]
fn bmff_3gpp_mime_too_short_returns_none() {
    let buf = [0u8; 10];
    assert!(super::bmff_3gpp_mime(&buf).is_none());
}

// End-to-end：FakeBackend 喂 `ftyp3gp5` 头 → Exif::open 走 3gpp fallback，
// 让 is_media() 通过门槛（之前 7 个「录像NNNN.mp4」3GP 文件残留场景）。
#[test]
fn open_uses_3gpp_fallback_for_3gp_brand() {
    use super::super::uri::Location;
    use crate::adapters::backend::fake::FakeBackend;
    use std::sync::Arc;

    let mut bytes = vec![0u8, 0, 0, 0x1c];
    bytes.extend_from_slice(b"ftyp3gp5");
    bytes.resize(256, 0);

    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-mem/clip.mp4"));
    fake.add_file(loc.clone(), bytes);

    let backend: Arc<dyn super::super::backend::Backend> = fake;
    let exif = Exif::open(&loc, &backend, utc()).unwrap();
    assert_eq!(exif.mime_type(), "video/3gpp");
    assert!(exif.is_media());
}
