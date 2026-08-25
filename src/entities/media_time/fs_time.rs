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

#[must_use]
pub fn from_modified(modified: Option<SystemTime>) -> Option<Candidate> {
    let t = modified?;
    let secs_u64 = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    convert_secs_to_candidate(secs_u64)
}

/// 把 u64 epoch 秒转 Candidate（仅 `FsMtime` 来源）。三段守护（`u64` → `i64` `try_from` +
/// `TimeDelta::try_seconds` + `DateTime::checked_add_signed`）的 Err arm 对正常 `SystemTime`
/// （Linux 内部用 i64 `tv_sec` 承载；Windows `FILETIME` 100ns 上限远低于 `i64::MAX` 秒）
/// 不可触发；不沿用 [`super::epoch_to_candidate`] 因为它把 `secs == 0` 视为未填 None，
/// 而 fs mtime = 1970-01-01 是合法值（fixture / 测试 `FakeBackend` 默认 mtime）。
/// `coverage(off)` 与 CLAUDE.md「难测/不可达分支」一致：调用入口由
/// `from_modified` 单测覆盖，本 helper 的 Err arm 物理不可达。
fn convert_secs_to_candidate(secs_u64: u64) -> Option<Candidate> {
    let signed = i64::try_from(secs_u64).ok()?;
    let delta = TimeDelta::try_seconds(signed)?;
    let utc = DateTime::<Utc>::UNIX_EPOCH.checked_add_signed(delta)?;
    Some(Candidate {
        utc,
        offset: None,
        source: Source::FsMtime,
        inferred_offset: false,
    })
}

#[cfg(test)]
#[path = "fs_time_tests.rs"]
mod tests;
