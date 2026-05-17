// docs/media-time-detection.md §七：判定结果的完整字段。

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::Utc;

use super::priority::Priority;
use super::priority::Source;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaTimeDecision {
    pub utc: DateTime<Utc>,
    pub offset: Option<FixedOffset>,
    pub priority: Priority,
    pub source: Source,
    pub inferred_offset: bool,
    pub confidence: Confidence,
    pub conflicts: Vec<Conflict>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    High,
    /// spec §5.3：1995 之前的时间被采纳但应人工复核。
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub other_utc: DateTime<Utc>,
    pub other_source: Option<Source>,
    pub diff_secs: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// spec §6：P0 vs GPS UTC 差值 > 24h
    GpsOver24h,
    /// spec §6：P0 vs 文件名解析差值 > 1d
    FilenameOver1Day,
    /// spec §6：mtime < P0 且差距较大，仅提示
    MtimeMuchEarlierThanP0,
}
