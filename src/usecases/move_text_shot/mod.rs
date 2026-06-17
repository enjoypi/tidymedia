//! `move-text-shot` 子命令：按 OCR 文本检测命中筛选图像，保留相对源 root
//! 的目录结构移到 output 下。
//!
//! 与 `copy/move` 子命令的区别：
//! - 归档维度是「图像内容含文本与否」，不是「拍摄时间」（不读 EXIF / `archive_template`）
//! - 目标路径 = `output / (src_path - source_root)`（相对路径保留）
//! - 仅处理 image MIME；非 image 跳过
//! - 默认移动（remove=true）；支持 `--dry-run`
//!
//! 依赖倒置：OCR 检测通过 `&dyn TextDetector` 注入，usecase 不感知 tract / 模型路径。

mod report;
mod run;

pub use report::MoveTextShotReport;
pub use run::move_text_shot;
