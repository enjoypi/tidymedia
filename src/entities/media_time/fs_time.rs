// docs/media-time-detection.md §二.P4 / §五.10：mtime 兜底。
// 只取 mtime；btime/ctime 在 spec 中明确不可用：
//   - btime/birthtime 在复制后变成"复制时刻"
//   - ctime 是 inode 元数据变更时间，与拍摄无关
//
// 设计：函数接 Option<SystemTime>，调用方用 `meta.modified().ok()` 把 Result → Option
// （`.ok()` 是无分支的统一映射，便于把 None/Err 路径在测试里通过传 None 触发）。

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;

use super::candidate::Candidate;
use super::priority::Source;

pub fn from_modified(modified: Option<SystemTime>) -> Option<Candidate> {
    let t = modified?;
    let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(Candidate {
        utc: DateTime::<Utc>::UNIX_EPOCH + TimeDelta::seconds(secs),
        offset: None,
        source: Source::FsMtime,
        inferred_offset: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn none_input_returns_none() {
        assert!(from_modified(None).is_none());
    }

    #[test]
    fn before_unix_epoch_returns_none() {
        let before = UNIX_EPOCH.checked_sub(Duration::from_secs(1)).unwrap();
        assert!(from_modified(Some(before)).is_none());
    }

    #[test]
    fn at_epoch_zero_ok() {
        let c = from_modified(Some(UNIX_EPOCH)).unwrap();
        assert_eq!(c.utc.timestamp(), 0);
        assert_eq!(c.source, Source::FsMtime);
        assert_eq!(c.offset, None);
        assert!(!c.inferred_offset);
    }

    #[test]
    fn future_systemtime_is_kept() {
        let t = UNIX_EPOCH + Duration::from_secs(1_704_110_400);
        let c = from_modified(Some(t)).unwrap();
        assert_eq!(c.utc.timestamp(), 1_704_110_400);
    }
}
