//! `cull` 命令的结构化报告。`serde::Serialize` 让 dispatch 层写 JSON 落盘。

use serde_derive::Serialize;

use crate::usecases::report::ReportError;

/// `cull` 子命令整体报告。
#[derive(Debug, Default, Serialize)]
pub struct CullReport {
    /// walker 触达的源端文件总数（含被识别为非媒体而跳过、解码失败等）。
    pub scanned: usize,
    /// pHash 分组后含 ≥ 2 张相似照片的组数（单图组不算入 grouped）。
    pub grouped: usize,
    /// 选出的 best 照片数量（每组一张 = grouped）。
    pub best_count: usize,
    /// 标记为 culled 的总数（应搬到 group 目录）。
    pub culled_count: usize,
    /// 实际搬迁成功数（`dry_run` 时 = 0）。
    pub moved: usize,
    /// 失败计数（任一阶段 IO/解码/模型 Err）。
    pub failed: usize,
    pub dry_run: bool,
    pub errors: Vec<ReportError>,
    pub groups: Vec<GroupReport>,
}

/// 单个相似组的详细：最佳源 + group 目录里的副本路径 + culled 列表 + 评分细节。
#[derive(Debug, Serialize)]
pub struct GroupReport {
    /// 1 起始，按选出顺序递增。
    pub group_id: usize,
    /// 源处最佳照片绝对路径（不动）。
    pub best_source: String,
    /// group 目录里的 `BEST_<basename>` 副本绝对路径。
    pub best_dest: String,
    /// 组内被搬走的劣质副本列表。
    pub culled: Vec<CulledEntry>,
    /// 最佳照片的综合评分明细。
    pub score_breakdown: ScoreBreakdown,
}

/// 单张被搬走的劣质副本：源路径 + group 目录里的 dst 路径 + 综合评分。
#[derive(Debug, Serialize)]
pub struct CulledEntry {
    pub source_path: String,
    pub dest_path: String,
    pub score: f32,
}

/// 综合评分明细：清晰度 / 闭眼惩罚 / 微笑加分 / 总分。
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ScoreBreakdown {
    pub sharpness: f32,
    pub blink_penalty: f32,
    pub smile_bonus: f32,
    pub total: f32,
}
