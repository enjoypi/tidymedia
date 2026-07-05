//! `move-text-shot` 子命令的 JSON 报告值对象。

use serde_derive::Serialize;

use crate::usecases::report::ReportError;

/// move-text-shot 操作报告。各计数维度互不重叠：`scanned` = walk 触达 entry 总数
/// （File + Dir + Other，与 `CopyReport` 口径一致，CLAUDE.md 同步检查点）；
/// `image_files` = size ≤ `max_image_bytes` 且 MIME sniff 命中 image 的文件数；
/// `ocr_hits` = detector 判命中数；`moved` = 实际搬移数（含 `dry_run` would-move
/// 与 `deduplicated` 幂等 skip 后删源）；`failed` = 报错数。
///
/// 不变量：
/// - `image_files + skipped_non_image + skipped_too_large + failed(walk 阶段) ≤ scanned`
/// - `ocr_hits + skipped_no_text ≤ image_files`
/// - `moved + deduplicated + failed(move 阶段) ≤ ocr_hits`
#[derive(Debug, Default, Serialize)]
pub struct MoveTextShotReport {
    /// walk 触达 entry 总数（含 Dir/Other/File / 被 MIME 过滤跳过 / 读不到）。
    pub scanned: usize,
    /// size ≤ `max_image_bytes` 且 MIME sniff 命中 image/* 的文件数。
    pub image_files: usize,
    /// detector 判命中（含文本）的文件数。
    pub ocr_hits: usize,
    /// 实际搬移文件数；`dry_run` 模式下亦累计 would-move。
    pub moved: usize,
    /// target 已存在且双侧 SHA-512 相等 → 视为幂等命中，删源计入此字段
    /// （不再重复移动），让二次跑同 source 得同结果不产 `_N` 副本。
    pub deduplicated: usize,
    /// MIME 非 image/* 跳过数。
    pub skipped_non_image: usize,
    /// detector 判未命中（不含文本）跳过数。
    pub skipped_no_text: usize,
    /// `entry.size > backend.ocr.max_image_bytes` 前置跳过数（防 OOM 硬门）。
    pub skipped_too_large: usize,
    /// 任意阶段失败计数（读字节失败 / detector Err / 命名耗尽 / 移动失败 / walker Err）。
    pub failed: usize,
    /// `errors` Vec 是否因 soft-cap 截断；`true` = 存在未记入 `errors` 的失败项，
    /// 用户应看 `failed` 总数与日志。
    pub errors_truncated: bool,
    pub dry_run: bool,
    pub errors: Vec<ReportError>,
    /// use case 入口到 return 的 wall-clock 耗时（毫秒）。供 AI 分析。
    pub duration_ms: u64,
}
