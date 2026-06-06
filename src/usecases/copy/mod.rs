//! copy use case：主流程编排（run）/ 单文件操作（ops）/ 命名策略（naming）子模块。
//! 对外路径（`usecases::copy::{copy, Source}`）经 re-export 保持不变。

pub(super) mod naming;
pub(super) mod ops;
pub(super) mod run;

pub(crate) use self::run::{Source, copy_with_sidecar};

// 测试经 `super::super::*` glob 访问的内部项（私有 use 对子模块可见，生产侧不暴露）。
#[cfg(test)]
use self::naming::{any_non_english, extract_valuable_name, generate_unique_name};
#[cfg(test)]
use self::ops::do_copy;
#[cfg(test)]
use self::run::copy;
#[cfg(test)]
use self::run::{
    CopyOpts, canonical_prefix, chrono_offset_from_hours, offset_from_hours, summary_result,
};
#[cfg(test)]
use crate::entities::file_info::Info;
#[cfg(test)]
use camino::Utf8Path;
#[cfg(test)]
use time::UtcOffset;

#[cfg(test)]
#[path = "copy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "copy_advanced_tests.rs"]
mod advanced_tests;

#[cfg(test)]
#[path = "copy_generate_tests.rs"]
mod generate_tests;

#[cfg(test)]
#[path = "copy_overlap_tests.rs"]
mod overlap_tests;
