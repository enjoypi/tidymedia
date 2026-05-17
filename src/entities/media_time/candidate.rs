// docs/media-time-detection.md §七：每个候选携带来源、时区、推断标记。

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::Utc;

use super::priority::Priority;
use super::priority::Source;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub utc: DateTime<Utc>,
    /// 已知或推断的时区偏移；None 表示来源本身无时区语义（如 MKV DateUTC、Unix 毫秒）。
    pub offset: Option<FixedOffset>,
    pub source: Source,
    /// true 表示 offset 是由调用方默认时区推断而非来源原生写明（spec §四）。
    pub inferred_offset: bool,
}

impl Candidate {
    pub fn priority(&self) -> Priority {
        self.source.priority()
    }
}
