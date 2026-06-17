//! `move-text-shot` 子命令的 JSON 报告值对象。

use serde_derive::Serialize;

use crate::usecases::report::ReportError;

/// move-text-shot 操作报告。各计数维度互不重叠：`scanned` = walk 触达 file 总数；
/// `image_files` = MIME 过滤后的图像数；`ocr_hits` = detector 判命中数；
/// `moved` = 实际搬移数（含 `dry_run` 的 would-move）；`failed` = 报错数。
///
/// 不变量：`image_files <= scanned`；`ocr_hits + skipped_no_text <= image_files`；
/// `moved + failed <= ocr_hits`（detector 命中后才进入移动尝试；同名耗尽落 `failed`）。
#[derive(Debug, Default, Serialize)]
pub struct MoveTextShotReport {
    /// walk 触达文件总数（含被 MIME 过滤跳过的非 image / 读不到的）。
    pub scanned: usize,
    /// 经 MIME 嗅探确认为 image/* 的文件数。
    pub image_files: usize,
    /// detector 判命中（含文本）的文件数。
    pub ocr_hits: usize,
    /// 实际搬移文件数；`dry_run` 模式下亦累计 would-move。
    pub moved: usize,
    /// MIME 非 image/* 跳过数。
    pub skipped_non_image: usize,
    /// detector 判未命中（不含文本）跳过数。
    pub skipped_no_text: usize,
    /// 任意阶段失败计数（读字节失败 / detector Err / 命名耗尽 / 移动失败）。
    pub failed: usize,
    pub dry_run: bool,
    pub errors: Vec<ReportError>,
}
