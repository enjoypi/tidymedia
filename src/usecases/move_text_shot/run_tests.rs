#![expect(
    clippy::doc_markdown,
    reason = "既有测试 doc 注释批量描述性中文夹标识符，clippy 1.95 `doc_markdown` 扩大后大量新增命中；测试文档不构成公开 API 契约，批量加反引号 ROI 低"
)]
//! `move_text_shot` 主流程单元测试：`FakeBackend` + `FakeTextDetector` 覆盖所有分支。
//!
//! 覆盖目标（与 plan 文件「branch miss 高危点」一一对应）：
//! - `is_image` / non-image MIME 过滤
//! - detector `Ok(true)` / `Ok(false)` / `Err` 三态
//! - `unique_name` 冲突 + 耗尽
//! - source ⊆ output overlap
//! - output ⊂ source walk-skip
//! - `dry_run` vs 真跑
//! - `read_all` Err（注入 reader error）
//! - 同 scheme rename fast-path vs 跨 scheme stream
//! - `split_stem_ext` / `relative_to` / `target_dir` 纯函数边界

use super::*;
use crate::adapters::backend::fake::FakeBackend;
use crate::adapters::ocr::fake::FakeTextDetector;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::Error;
use crate::usecases::report::ERRORS_SOFT_CAP;
use camino::Utf8PathBuf;
use std::io;

// ---- 纯函数边界 ----

include!("run_part1_tests.rs");
include!("run_part2_tests.rs");
include!("run_part3_tests.rs");
include!("run_part4_tests.rs");
include!("run_part5_tests.rs");
include!("run_part6_tests.rs");
