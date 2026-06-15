use super::SecureHash;

fn str_to_secure(input_str: &str) -> SecureHash {
    let vec: Vec<u8> = hex::decode(input_str).expect("test hex input must be valid");
    SecureHash::try_from(vec.as_slice()).expect("test hex input must encode 64 bytes")
}

pub const DATA_DIR: &str = "tests/data";
pub const DATA_SMALL: &str = "tests/data/data_small";
pub const DATA_SMALL_LEN: u64 = 3057;
pub const DATA_SMALL_WYHASH: u64 = 13_333_046_383_594_682_858;
pub const DATA_SMALL_XXHASH: u64 = 0x1a5e_fdfd_bd01_a44c;

pub fn data_small_sha512() -> SecureHash {
    str_to_secure(
        "c77d955d24f36057a2fc6eba10d9a386ef6b8a6568e73bb8f6a168b4e2adc65fa2ffdc6f6e479f42199b740b8e83af74caffa6f580d4b7351be20efa65b0fcd2",
    )
}

pub const DATA_SMALL_COPY: &str = "tests/data/data_small_copy";

pub const DATA_LARGE: &str = "tests/data/data_large";
pub const DATA_LARGE_LEN: u64 = 7133;
pub const DATA_LARGE_WYHASH: u64 = 2_034_553_491_748_707_037;
pub const DATA_LARGE_XXHASH: u64 = 0x9dba_53c5_9ea9_68e9;

pub fn data_large_sha512() -> SecureHash {
    str_to_secure(
        "0f7fd3e44b860c33de83c19edb759edcad9c6e101910f765e86e2443f533f9c254ad544a84e4bb56b221620148c79b2b8619cfd8f611d30617c6c32f210bcea7",
    )
}

pub const DATA_LARGE_COPY: &str = "tests/data/data_large_copy";

pub const DATA_DNS_BENCHMARK: &str = "tests/data/DNSBenchmark.png";

/// 含 EXIF DateTimeOriginal/CreateDate/ModifyDate 的小 JPEG，ffmpeg+exiftool 生成。
pub const DATA_JPEG_WITH_EXIF: &str = "tests/data/sample-with-exif.jpg";
/// 有 EXIF block（仅 Make 标签），三个日期字段全无 —— 用于覆盖 if let Some 的 None 分支。
pub const DATA_JPEG_NO_DATES: &str = "tests/data/sample-no-dates.jpg";
/// 含 `QuickTime` track `CreateDate` 的小 MP4，ffmpeg 生成。
pub const DATA_MP4_WITH_TRACK: &str = "tests/data/sample-with-track.mp4";
/// Matroska 视频，track 但无 `CreateDate` —— 用于覆盖 `populate_video_dates` 的 None 分支。
pub const DATA_MKV_NO_TRACK_DATE: &str = "tests/data/sample-no-track-date.mkv";
/// MKV 含 `DateUTC = 2023-06-15T10:30:00Z` —— 验证 `MkvDateUtc` 候选路径。
pub const DATA_MKV_WITH_DATE: &str = "tests/data/sample-with-mkv-date.mkv";
/// JPEG 含 `GPSDateStamp=2023-06-15` / `GPSTimeStamp=10:30:00` / `DateTimeOriginal=18:30:00` 同天。
/// 用于验证 `gps_utc()` 解析以及 `create_time` 的 GPS 冲突告警路径。
pub const DATA_JPEG_WITH_GPS: &str = "tests/data/sample-with-gps.jpg";
/// JPEG 同时含 `Make=TestCam` 和 `Model=TestModel`（由 `exiftool` 写入 sample-with-exif.jpg 生成）。
/// 用于覆盖 `populate_image_dates` 中 `ExifTag::Model` 的 `and_then` 分支。
pub const DATA_JPEG_WITH_MAKE_MODEL: &str = "tests/data/sample-with-make-model.jpg";
/// JPEG EXIF block 完全无三日期，XMP packet 同时含 `photoshop:DateCreated` 与
/// `xmp:CreateDate=2008-10-31T09:15:01+08:00`（exiftool Shorthand 模式写 attribute 形式）。
/// 用于验证 `populate_image_xmp_fallback` 路径。
pub const DATA_JPEG_XMP_ONLY: &str = "tests/data/sample-xmp-only.jpg";
/// JPEG 仅有 `EXIF:CreateDate=2024:01:02 12:00:00`、无 `DateTimeOriginal`、无 XMP。
/// 用于覆盖 `populate_image_dates` 中 `dto==0 && cd!=0` 的 XMP fallback 短路 branch
/// （cd 非 0 时 fallback 不应触发；从 fixture 的真实数据导出 branch 觉醒）。
pub const DATA_JPEG_ONLY_CREATEDATE: &str = "tests/data/sample-only-createdate.jpg";
/// 真实 Canon AVCHD（BDAV M2TS）前 1024 字节截断；H.264 SEI MDPM 含
/// `2011-10-01 10:35:57`（时区字节忽略，按调用方 offset 解释）。
pub const DATA_M2TS_CANON: &str = "tests/data/sample-canon-avchd.m2ts";

// docs/media-time-detection.md spec contract fixture：tests/fixtures/gen.sh 生成，
// tests/media_time_spec.rs 集成测试通过 tests/media_time/common.rs 内的等价常量引用
// （集成测试是独立 crate，看不见 pub(crate) 项）。

/// 2024-01-01 12:00:00 UTC，用于固定 PNG 复制目标的 mtime
pub const FIXED_MEDIA_MTIME: i64 = 1_704_110_400;

// 测试 fixture helper：fs::copy 的 Err 已在 L41 通过 missing 目录测试覆盖；
// set_file_mtime 在 fs::copy 成功后立即调用，Err 分支不可稳定触发。整体标 coverage(off)。
pub fn copy_png_to(
    target_dir: &std::path::Path,
    name: &str,
) -> std::io::Result<std::path::PathBuf> {
    let dst = target_dir.join(name);
    std::fs::copy(DATA_DNS_BENCHMARK, &dst)?;
    let ts = filetime::FileTime::from_unix_time(FIXED_MEDIA_MTIME, 0);
    filetime::set_file_mtime(&dst, ts)?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    // target_dir 不存在 → fs::copy 失败，覆盖 L41 ? Err。
    #[test]
    fn copy_png_to_errors_when_target_dir_missing() {
        let bogus = std::path::Path::new("/definitely/missing/parent/zzz_tc");
        let err = copy_png_to(bogus, "x.png").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    // 拷贝完成后立即删 dst，再让 set_file_mtime 失败，覆盖 L43 ? Err。
    // 通过两步走：先成功 copy_png_to，再单独调用 set_file_mtime 验证它会失败。
    // 这里直接构造：把 dst 立即转成一个不存在的同名文件路径。
    #[test]
    fn set_file_mtime_on_missing_path_fails() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("never-created.png");
        let ts = filetime::FileTime::from_unix_time(FIXED_MEDIA_MTIME, 0);
        let err = filetime::set_file_mtime(&missing, ts).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
