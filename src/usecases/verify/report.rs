use std::collections::BTreeMap;

use serde_derive::Serialize;

use crate::usecases::report::ReportError;

/// verify 操作报告。`scanned` = 源端 walker 触达文件数（与 `CopyReport` 同口径）；
/// `compared` = 进入逐项对账的文件数；`mismatched` = 归档桶对账不一致的条目数；
/// `decision_failed` = 无 P0..P4 候选可裁决（`media_time_decision` 为 None）的文件数。
/// `dry_run` 恒 true：verify 只诊断不写盘。
#[derive(Debug, Default, Serialize)]
pub struct VerifyReport {
    pub scanned: usize,
    pub compared: usize,
    pub mismatched: usize,
    pub decision_failed: usize,
    /// 9 种诊断 pattern 命中计数，key 见 `diagnose.rs`；空 = 尚未诊断。
    pub pattern_counts: BTreeMap<String, usize>,
    pub entries: Vec<VerifyEntry>,
    pub errors: Vec<ReportError>,
    pub errors_truncated: bool,
    pub dry_run: bool,
    pub include_non_media: bool,
    pub duration_ms: u64,
}

/// 单个源文件的对账行。阶段逐步填充：骨架阶段只填决策相关字段，
/// 桶对账/内容比对/诊断在后续阶段补齐（未填字段保持 default）。
#[derive(Debug, Default, Serialize)]
pub struct VerifyEntry {
    pub source_path: String,
    /// 归档预测桶 `YYYY:MM`（由决策时间 + 配置时区推导）。
    pub actual_bucket: String,
    /// 决策优先级（P0..P4 / `(none)`）。
    pub chosen_priority: String,
    /// 决策来源（`media_time::Source` Debug 名 / `(none)`）。
    pub chosen_source: String,
    /// 冲突列表（`ConflictKind` Debug 名）。
    pub conflicts: Vec<String>,
    /// 注入 exiftool tsv 时第二实现给出的期望桶 `YYYY:MM`。
    pub exif_exp_bucket: Option<String>,
    /// 期望桶来源标签（`DTO`/`QTCreationDate`/`QTCreateDate`/`CreateDate`/`FsMtime`）。
    pub exif_from: Option<String>,
    /// 注入 tsv 的相机品牌/型号（skill MISMATCH 行的 make/model，供人工定位）。
    pub exif_make: Option<String>,
    pub exif_model: Option<String>,
    /// 文件名/相对路径抽出的日期桶 `YYYY:MM`（skill 的 name= 列，仅诊断不进决策）。
    pub filename_bucket: Option<String>,
    /// 期望桶与预测桶不一致（含未注入 tsv 时的内部一致性判定）。
    pub mismatch: bool,
    /// 内容比对裁定：`not_checked`/`exact_dup`/`pixel_same`/`rotated_same`/`absent`。
    pub duplicate_verdict: String,
    /// 命中的诊断 pattern 名。
    pub patterns: Vec<String>,
    /// 修补建议（exiftool 命令等），None = 无需修补。
    pub fix_suggestion: Option<String>,
}
