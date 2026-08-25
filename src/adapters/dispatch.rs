use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::adapters::cli::Commands;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::{Error, Result};
use crate::frameworks::detector::DefaultDetectorFactory;
use crate::usecases::cull::CullReport;
use crate::usecases::detector::DetectorFactory;
use crate::usecases::move_text_shot::MoveTextShotReport;
use crate::usecases::report::{CopyReport, FindReport};
use crate::usecases::verify::VerifyReport;

// 各子命令 dispatch 与公共 helper 拆到平级文件，保持本文件 ≤300 行。
#[path = "dispatch_commands.rs"]
mod commands;

use self::commands::{
    dispatch_copy_or_move, dispatch_cull, dispatch_find, dispatch_move_text_shot, dispatch_verify,
};

/// 子命令执行结果：Copy/Move 返回 [`CopyReport`]，Find 返回 [`FindReport`]，
/// `MoveTextShot` 返回 [`MoveTextShotReport`]，`Cull` 返回 [`CullReport`]，
/// `Verify` 返回 [`VerifyReport`]。
/// `tidy_with` 单一入口同时服务 CLI（丢弃返回）与 Android/mobile（消费 report）。
#[derive(Debug)]
pub enum CommandResult {
    Copy(CopyReport),
    Find(FindReport),
    MoveTextShot(MoveTextShotReport),
    Cull(CullReport),
    Verify(VerifyReport),
}

/// 用默认 backend / detector factory 跑命令；旧入口，等价于
/// `tidy_with(&DefaultBackendFactory, &DefaultDetectorFactory, ...)`。
///
/// # Errors
///
/// 当命令执行过程中发生 IO 错误、backend / detector 构造失败、业务逻辑出错，或
/// Copy/Move 出现非零 failed（部分文件复制失败）时返回 `Err`，让 CLI 退出码非 0
/// 让 CI/cron 脚本能区分"全部成功"与"部分失败"。
pub fn tidy(command: Commands) -> Result<()> {
    let result = tidy_with(&DefaultBackendFactory, &DefaultDetectorFactory, command)?;
    match result {
        CommandResult::Copy(report) if report.failed > 0 => {
            // (remove, doc_only) 区分四个子命令：错文案会让 CI 脚本误判子命令。
            let op = match (report.remove, report.doc_only) {
                (false, false) => "copy",
                (false, true) => "copy-doc",
                (true, false) => "move",
                (true, true) => "move-doc",
            };
            let past = if report.remove { "moved" } else { "copied" };
            Err(Error::Io(std::io::Error::other(format!(
                "{op} partial failure: {failed} failed, {ok} {past}, {ignored} ignored",
                failed = report.failed,
                ok = report.copied,
                ignored = report.ignored,
            ))))
        }
        CommandResult::MoveTextShot(report) if report.failed > 0 => {
            Err(Error::Io(std::io::Error::other(format!(
                "move-text-shot partial failure: {} failed, {} moved, {} skipped_no_text, \
                 {} skipped_non_image",
                report.failed, report.moved, report.skipped_no_text, report.skipped_non_image
            ))))
        }
        CommandResult::Cull(report) if report.failed > 0 => {
            Err(Error::Io(std::io::Error::other(format!(
                "cull partial failure: {} failed, {} moved, {} culled, {} grouped",
                report.failed, report.moved, report.culled_count, report.grouped
            ))))
        }
        CommandResult::Verify(report) if report.mismatched > 0 || report.decision_failed > 0 => {
            Err(Error::Io(std::io::Error::other(format!(
                "verify found {mismatched}/{compared} mismatched buckets, {unresolved} unresolved \
                 decisions, {scanned} scanned",
                mismatched = report.mismatched,
                compared = report.compared,
                unresolved = report.decision_failed,
                scanned = report.scanned,
            ))))
        }
        CommandResult::Copy(_)
        | CommandResult::Find(_)
        | CommandResult::MoveTextShot(_)
        | CommandResult::Cull(_)
        | CommandResult::Verify(_) => Ok(()),
    }
}

/// 注入版入口：调用方提供 [`BackendFactory`] 与 [`DetectorFactory`]，常用于集成测试
/// 用 fake 装配混合 scheme + 直接跑 Copy/Move/Find 而不加载 ONNX 模型。
/// 返回结构化 [`CommandResult`]：CLI 路径直接 `?` 丢弃，mobile 路径 match 取 report。
///
/// # Errors
///
/// 当 backend / detector 构造失败、IO 操作出错或业务逻辑出错时返回 `Err`。
#[expect(
    clippy::too_many_lines,
    reason = "六子命令 match 每 arm 是纯解构透传；拆分只会把 enum 解构搬到别处"
)]
pub fn tidy_with(
    factory: &dyn BackendFactory,
    detectors: &dyn DetectorFactory,
    command: Commands,
) -> Result<CommandResult> {
    match command {
        Commands::Copy {
            dry_run,
            include_non_media,
            sources,
            output,
            archive_template,
            report,
        } => dispatch_copy_or_move(
            factory,
            detectors,
            sources,
            output,
            dry_run,
            /* remove = */ false,
            include_non_media,
            /* doc_only = */ false,
            archive_template.as_deref(),
            report.as_deref(),
        ),
        Commands::Move {
            dry_run,
            include_non_media,
            sources,
            output,
            archive_template,
            report,
        } => dispatch_copy_or_move(
            factory,
            detectors,
            sources,
            output,
            dry_run,
            /* remove = */ true,
            include_non_media,
            /* doc_only = */ false,
            archive_template.as_deref(),
            report.as_deref(),
        ),
        Commands::CopyDoc {
            dry_run,
            sources,
            output,
            archive_template,
            report,
        } => dispatch_copy_or_move(
            factory,
            detectors,
            sources,
            output,
            dry_run,
            /* remove = */ false,
            /* include_non_media = */ false,
            /* doc_only = */ true,
            archive_template.as_deref(),
            report.as_deref(),
        ),
        Commands::MoveDoc {
            dry_run,
            sources,
            output,
            archive_template,
            report,
        } => dispatch_copy_or_move(
            factory,
            detectors,
            sources,
            output,
            dry_run,
            /* remove = */ true,
            /* include_non_media = */ false,
            /* doc_only = */ true,
            archive_template.as_deref(),
            report.as_deref(),
        ),
        Commands::Find {
            secure,
            sources,
            output,
            report,
        } => dispatch_find(factory, sources, output, secure, report.as_deref()),
        Commands::MoveTextShot {
            dry_run,
            sources,
            output,
            report,
        } => dispatch_move_text_shot(
            factory,
            detectors,
            sources,
            output,
            dry_run,
            report.as_deref(),
        ),
        Commands::Cull {
            dry_run,
            sources,
            output,
            phash_max,
            report,
        } => dispatch_cull(
            factory,
            detectors,
            sources,
            output,
            dry_run,
            phash_max,
            report.as_deref(),
        ),
        Commands::Verify {
            sources,
            output,
            include_non_media,
            exif_tsv,
            phash_max,
            report,
        } => dispatch_verify(
            factory,
            sources,
            output,
            include_non_media,
            exif_tsv.as_deref(),
            phash_max,
            report.as_deref(),
        ),
    }
}
