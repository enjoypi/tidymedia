//! `cull` 子命令：pHash 相似分组 + 4 模型印证人脸质量评分，挑最佳留源，
//! 劣质副本搬到 `output/<相对路径>/group-NNN/` + 最佳照片复制一份加 `BEST_` 前缀。
//!
//! 人脸对齐、身份聚类、眼部状态与清晰度评分分别由 `face_align`、
//! `identity_cluster`、`face_scoring` 等模块协作完成。

mod commit;
mod crop;
mod face_align;
mod face_scoring;
mod group_writer;
mod identity_cluster;
pub(crate) mod phash;
mod report;
mod run;
mod scan;
mod score;
mod sharpness;
mod util;

#[doc(hidden)]
pub use group_writer::best_effort_remove_partial_dst;
pub use report::{CullReport, CulledEntry, GroupReport, ScoreBreakdown};
pub use run::cull;
