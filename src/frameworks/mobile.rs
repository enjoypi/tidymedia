//! Android / 移动端 uniffi 绑定层。
//!
//! 仅在 `--features android-app` 启用时编译；通过 `uniffi::setup_scaffolding!()`
//! 在 lib.rs 顶层一次生成 FFI scaffolding，本模块用 `#[uniffi::export]` 暴露
//! 简化的 dry-run / run / find-duplicates 入口给 Kotlin 调用。
//!
//! 设计哲学（与 [CLI 入口] 对齐）：
//! - Kotlin 端把 SAF 选到的目录翻成本地 file path（Android 11+ 可用
//!   `/storage/emulated/0/...` 或 `MediaStore` 投影），传 `String` 给 Rust；
//!   Rust 端走与 CLI 完全一致的路径：`DefaultBackendFactory`
//! - 没有专门的 mobile Use Case；保留 Single Responsibility（mobile 只做 FFI 适配）
//! - `sources` 字符串接受与 CLI 相同的 URI 语法（`smb://...` / `adb://...` /
//!   本地路径），由 `Location::from_str` 解析，Kotlin 端无需感知 backend 细节
//!
//! [CLI 入口]: crate::run_cli

use std::str::FromStr;

use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::adapters::cli::Commands;
use crate::adapters::dispatch::{CommandResult, tidy_with};
use crate::entities::uri::Location;
use crate::usecases::report::{CopyReport, FindReport};

/// 一次 tidy copy 调用的统计。UI 用这些字段显示进度。
#[derive(uniffi::Record, Clone, Debug)]
pub struct TidyStats {
    /// 扫到的源端文件总数（含被识别为非媒体而跳过的）
    pub total_scanned: u32,
    /// 实际写入目标端的文件数（`dry_run=true` 时永远为 0）
    pub copied: u32,
    /// 被跳过的文件数（重复 / 非媒体）
    pub ignored: u32,
    /// 复制失败的文件数
    pub failed: u32,
    /// 操作状态：正常完成 = `"ok"` / dry-run = `"dry-run ok"` / 被 Err 截断 = error 文案
    pub status: String,
}

/// find-duplicates 操作的简要报告。
#[derive(uniffi::Record, Clone, Debug)]
pub struct MobileFindReport {
    /// 入索引的源端文件总数（包括非重复文件）。
    pub scanned: u32,
    /// 重复组数量
    pub group_count: u32,
    /// 每组的文件路径列表（保留组边界；旧 CSV 拼接对路径含逗号场景错乱，故用嵌套序列）。
    pub groups: Vec<MobileDuplicateGroup>,
    /// 流式哈希过程中累计读取的字节数。
    pub bytes_read: u64,
}

/// 一组重复文件：size（组内共享，下游按 size 过滤/排序用）+ 路径集合。
/// uniffi 0.31 原生支持嵌套 Record sequence，跨 FFI 直接映射。
#[derive(uniffi::Record, Clone, Debug)]
pub struct MobileDuplicateGroup {
    /// 组内每个文件的字节数（同组 size 相同）。
    pub size_bytes: u64,
    pub paths: Vec<String>,
}

/// uniffi 暴露给 Kotlin 的统一错误。
/// 将 [`crate::Error`] 收敛成单一变体携带文案；UI 仅展示文案不做结构化匹配。
/// 字段名故意用 `text` 而非 `message`：uniffi 0.31 在 Kotlin 端把 `message`
/// 字段渲染成 `val message: String`，会与 `kotlin.Exception.message` 撞名导致编译失败。
#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum TidyError {
    #[error("{text}")]
    Generic { text: String },
}

impl From<crate::Error> for TidyError {
    fn from(e: crate::Error) -> Self {
        Self::Generic {
            text: format!("{e}"),
        }
    }
}

/// Dry-run：扫源 / 找重复，但不写目标，不删源。
/// Kotlin 调用约定：`src` 接受本地绝对路径或 URI（`smb://...` / `adb://...`），
/// `output` 同样是路径或 URI，dry-run 下不实际写。
///
/// # Errors
///
/// 当扫描源、解析路径或底层执行失败时返回 `TidyError`。
#[uniffi::export]
// uniffi 0.31 FFI 边界要求 owned String；clippy::needless_pass_by_value 只在
// android-app feature 启用时触发，跨 feature 差异须用 #[allow] 不用 #[expect]。
#[allow(
    clippy::needless_pass_by_value,
    reason = "uniffi 0.31 FFI export 强制 owned String 入参"
)]
pub fn tidy_dry_run(src: String, output: String) -> Result<TidyStats, TidyError> {
    crate::install_config_loader();
    run_copy_internal(&src, &output, /* dry_run = */ true)
}

/// 真实跑：扫源 → 复制非重复媒体到 output，不删源（move 模式不在 P1 范围）。
///
/// # Errors
///
/// 当扫描源、解析路径或底层执行失败时返回 `TidyError`。
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "uniffi 0.31 FFI export 强制 owned String 入参"
)]
pub fn tidy_run(src: String, output: String) -> Result<TidyStats, TidyError> {
    crate::install_config_loader();
    run_copy_internal(&src, &output, /* dry_run = */ false)
}

/// 查找重复文件。`sources` 接受本地路径或 URI 列表，`secure` 为 true 时用 SHA-512。
///
/// # Errors
///
/// 当解析路径或底层执行失败时返回 `TidyError`。
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "uniffi 0.31 FFI export 强制 owned Vec<String> 入参"
)]
pub fn tidy_find_duplicates(
    sources: Vec<String>,
    secure: bool,
) -> Result<MobileFindReport, TidyError> {
    crate::install_config_loader();
    let locs = parse_locations(sources)?;
    let result = tidy_with(
        &DefaultBackendFactory,
        Commands::Find {
            secure,
            sources: locs,
            output: None,
            report: None,
        },
    )?;
    // .map 而非 `?`+Ok：expect_find 的 Err arm 在本调用点不可达（Find 命令必返
    // Find 结果），`?` 会留下永不触发的 Err 传播 region。
    expect_find(result).map(mobile_report_from)
}

/// 版本号给 UI 显示用，便于排查"App 里 Rust core 哪个版本"。
#[uniffi::export]
#[must_use]
pub fn tidymedia_version() -> String {
    // 装 config loader 不强求 — 版本号不消费 config — 但保持「每个 FFI export 顶部
    // 都装一遍」的统一约定，避免后续新增 export 时漏装；OnceLock 装载是幂等的。
    crate::install_config_loader();
    env!("CARGO_PKG_VERSION").to_string()
}

// 解析 URI 字符串列表为 Location 列表；任意一个失败即返回 TidyError。
fn parse_locations(sources: Vec<String>) -> Result<Vec<Location>, TidyError> {
    sources
        .into_iter()
        .map(|s| {
            Location::from_str(&s).map_err(|e| TidyError::Generic {
                text: format!("invalid source URI {s:?}: {e}"),
            })
        })
        .collect()
}

fn run_copy_internal(src: &str, output: &str, dry_run: bool) -> Result<TidyStats, TidyError> {
    let src_loc = Location::from_str(src).map_err(|e| TidyError::Generic {
        text: format!("invalid source URI {src:?}: {e}"),
    })?;
    let out_loc = Location::from_str(output).map_err(|e| TidyError::Generic {
        text: format!("invalid output URI {output:?}: {e}"),
    })?;
    let result = tidy_with(
        &DefaultBackendFactory,
        Commands::Copy {
            dry_run,
            include_non_media: false,
            sources: vec![src_loc],
            output: out_loc,
            archive_template: None,
            report: None,
        },
    )?;
    // 同 tidy_find_duplicates：.map 避免调用点不可达的 `?` Err region。
    expect_copy(result).map(|report| stats_from(&report, dry_run))
}

// 纯映射：CopyReport → TidyStats，status 文案由 copy_status 决定。
fn stats_from(report: &CopyReport, dry_run: bool) -> TidyStats {
    TidyStats {
        total_scanned: u32::try_from(report.scanned).unwrap_or(u32::MAX),
        copied: u32::try_from(report.copied).unwrap_or(u32::MAX),
        ignored: u32::try_from(report.ignored).unwrap_or(u32::MAX),
        failed: u32::try_from(report.failed).unwrap_or(u32::MAX),
        status: copy_status(dry_run, report.failed),
    }
}

// 纯映射：FindReport → MobileFindReport（保留组边界，不做 CSV 展平）。
fn mobile_report_from(report: FindReport) -> MobileFindReport {
    MobileFindReport {
        scanned: u32::try_from(report.scanned).unwrap_or(u32::MAX),
        group_count: u32::try_from(report.groups.len()).unwrap_or(u32::MAX),
        groups: report
            .groups
            .into_iter()
            .map(|g| MobileDuplicateGroup {
                size_bytes: g.size,
                paths: g.paths,
            })
            .collect(),
        bytes_read: report.bytes_read,
    }
}

// 收敛 dispatch 返回的 CommandResult 到 CopyReport；Copy 命令必返 Copy 结果，
// 错配属内部错误——FFI 边界不 panic，改返 Err 让 Kotlin 端展示文案。
fn expect_copy(result: CommandResult) -> Result<CopyReport, TidyError> {
    let CommandResult::Copy(report) = result else {
        return Err(TidyError::Generic {
            text: "internal error: copy command returned non-copy result".into(),
        });
    };
    Ok(report)
}

// expect_copy 的 Find 对偶。
fn expect_find(result: CommandResult) -> Result<FindReport, TidyError> {
    let CommandResult::Find(report) = result else {
        return Err(TidyError::Generic {
            text: "internal error: find command returned non-find result".into(),
        });
    };
    Ok(report)
}

// 与 CLI 路径（copy.rs）对齐：failed>0 → partial；dry-run 走独立 status 文案。
fn copy_status(dry_run: bool, failed: usize) -> String {
    match (dry_run, failed) {
        (true, 0) => "dry-run ok".to_string(),
        (true, n) => format!("dry-run partial ({n} failed)"),
        (false, 0) => "ok".to_string(),
        (false, n) => format!("partial ({n} failed)"),
    }
}

#[cfg(test)]
#[path = "mobile_tests.rs"]
mod tests;
