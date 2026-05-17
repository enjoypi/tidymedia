// docs/media-time-detection.md 契约测试共享 helper。
// 主要服务于程序化构造 Candidate 的测试；fixture 路径常量按需在用到的子模块里
// 直接写字面量，避免维护多份 src/entities/test_common.rs 镜像。

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::TimeZone;
use chrono::Utc;
use std::path::Path;

pub fn utc_offset() -> FixedOffset {
    FixedOffset::east_opt(0).expect("0 offset is valid")
}

pub fn east8() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("+8 offset is valid")
}

/// 固定 now 用于 filter 计算，避免 wallclock 漂移让测试不稳定。
pub fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 17, 0, 0, 0)
        .single()
        .expect("fixed timestamp is valid")
}

pub fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .expect("epoch second is valid")
}

#[allow(dead_code)]
pub fn set_mtime(path: &Path, secs: i64) {
    let ts = filetime::FileTime::from_unix_time(secs, 0);
    filetime::set_file_mtime(path, ts).expect("can set mtime on test fixture");
}
