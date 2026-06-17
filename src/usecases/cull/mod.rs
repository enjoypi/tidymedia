//! `cull` 子命令：pHash 相似分组 + 4 模型印证人脸质量评分，挑最佳留源，
//! 劣质副本搬到 `output/<相对路径>/group-NNN/` + 最佳照片复制一份加 `BEST_` 前缀。
//!
//! **首版骨架**（plan F 节风险 #1 已明示需 e2e 真跑验证 + 真实模型对接）：
//! 仅注入 4 个 detector + 实际只调 SCRFD 作为「是否检测到人脸」信号，加 pHash + 全图
//! 清晰度做评分；完整 `ArcFace` 5 点对齐 + `MobileFaceNet` embedding + 跨图身份聚类 +
//! `FaceMesh` EAR + `EyeState` 双印证流水线，留 e2e 步骤 6 真跑后补足（对应模块
//! `face_align` / `identity_cluster` / `face_scoring` 届时新增）。

mod face_align;
mod group_writer;
mod identity_cluster;
mod phash;
mod report;
mod run;
mod sharpness;

pub use report::{CullReport, CulledEntry, GroupReport, ScoreBreakdown};
pub use run::cull;
