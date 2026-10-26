//! 拍摄时间裁决：P0→P4 优先级 + 软阈值 fallback。
//! 自由函数（`Info::create_time` / `media_time_decision` 薄包装转发）+ P4 fs 兜底。
//! 子模块可访问父模块 `Info` 的私有字段。

use std::time::Duration;
use std::time::SystemTime;

use camino::Utf8Path;
use chrono::FixedOffset;
use chrono::TimeZone;
use chrono::Utc;
use tracing::warn;

use super::Info;
use crate::entities::{exif, media_time};

pub fn create_time(
    info: &Info,
    valid_threshold_secs: u64,
    default_offset: FixedOffset,
) -> SystemTime {
    let fs_fallback = pick_fs_fallback(info.meta.modified, info.meta.created);
    let decision = media_time_decision(info, default_offset);
    // resolve 返回 None（候选全部被过滤）与"低于阈值"走同一条 fallback 路径，
    // 避免在 create_time 里多一条不可稳定触发的分支。
    let secs = decision.map_or(0, |d| d.utc.timestamp());
    if secs > 0 && secs.cast_unsigned() >= valid_threshold_secs {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs.cast_unsigned())
    } else {
        fs_fallback
    }
}

/// 完整拍摄时间决策（P0→P4 优先级 + 冲突列表），供归档决策与 `verify` 对账
/// 共用。返回 resolve 原样 decision，不做软阈值过滤（调用方按需解释）。
pub fn media_time_decision(
    info: &Info,
    default_offset: FixedOffset,
) -> Option<media_time::MediaTimeDecision> {
    let modified = info.meta.modified;
    // P2 文件名中的 naive 时间按 default_offset（配置时区）解释，与 EXIF naive
    // 同口径——按 UTC 解释会让月末晚间拍摄的文件 +offset 后跨月归错桶；
    // P0/P1 的 epoch 已在 EXIF 解析层（from_path_with_offset）按配置时区转换完毕，
    // 这里的 offset 对其仅作候选元数据。
    let gps_utc = info.exif.as_ref().and_then(exif::Exif::gps_utc);
    // ModifyDate 不进候选，仅作多数派仲裁的 re-save 旁证；epoch 已在
    // EXIF 解析层按配置时区转换，0 = 缺失。
    // 与 epoch_to_candidate 同口径：i64::try_from 守护 u64>i64::MAX 折回负数
    // 致 timestamp_opt 返虚假 1969 时间戳被多数派误用为 re-save 旁证。
    let modify_date_utc = info
        .exif
        .as_ref()
        .map(exif::Exif::exif_modify_date)
        .filter(|&s| s > 0)
        .and_then(|s| i64::try_from(s).ok())
        .and_then(|s| Utc.timestamp_opt(s, 0).single());
    let mut candidates = match info.exif.as_ref() {
        Some(exif) => media_time::candidates_from_exif(exif, default_offset),
        None => Vec::new(),
    };
    // P2：文件名启发式（IMG_/DSC_/Screenshot_/毫秒戳等）。
    candidates.extend(media_time::candidates_from_filename(
        Utf8Path::new(info.full_path.as_str()),
        default_offset,
    ));
    // P3：adapters 层发现并注入的 sidecar 候选（XMP / Google Takeout）。
    candidates.extend(info.extra_candidates.iter().copied());
    // P4。Option<Candidate> 实现 IntoIterator → extend 不引入 if-let 分支。
    candidates.extend(media_time::fs_time::from_modified(modified));

    let decision = media_time::resolve(candidates, gps_utc, modify_date_utc, Utc::now());
    // 冲突优先告警，不静默修正。
    if let Some(ref d) = decision
        && !d.conflicts.is_empty()
    {
        warn!(
            feature = "file_info",
            operation = "resolve_time",
            file = %info.full_path,
            conflicts = ?d.conflicts,
            "media time candidates conflict"
        );
    }
    decision
}

pub fn pick_fs_fallback(modified: Option<SystemTime>, created: Option<SystemTime>) -> SystemTime {
    modified.or(created).unwrap_or(SystemTime::UNIX_EPOCH)
}
