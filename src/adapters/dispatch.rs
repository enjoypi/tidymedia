use crate::adapters::cli::Commands;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::{Error, Result};
use crate::frameworks::backend::DefaultBackendFactory;
use crate::frameworks::detector::DefaultDetectorFactory;
use crate::usecases::detector::DetectorFactory;
use crate::usecases::move_text_shot::MoveTextShotReport;
use crate::usecases::report::{CopyReport, FindReport};
use crate::usecases::verify::VerifyReport;

#[path = "dispatch_commands.rs"]
mod commands;

use self::commands::{
    dispatch_copy_or_move, dispatch_find, dispatch_move_text_shot, dispatch_verify,
};

#[derive(Debug)]
pub enum CommandResult {
    Copy(CopyReport),
    Find(FindReport),
    MoveTextShot(MoveTextShotReport),
    Verify(VerifyReport),
}

#[expect(
    clippy::missing_errors_doc,
    reason = "错误经统一 Result 传播，失败计数在 CommandReport"
)]
pub fn tidy(command: Commands) -> Result<()> {
    let result = tidy_with(&DefaultBackendFactory, &DefaultDetectorFactory, command)?;
    match result {
        CommandResult::Copy(report) if report.failed > 0 => {
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
        | CommandResult::Verify(_) => Ok(()),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "各子命令 match 每 arm 是纯解构透传；拆分只会把 enum 解构搬到别处"
)]
#[expect(
    clippy::missing_errors_doc,
    reason = "错误经统一 Result 传播，调用方按 CommandResult 处理"
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
            false,
            include_non_media,
            false,
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
            true,
            include_non_media,
            false,
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
            false,
            false,
            true,
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
            true,
            false,
            true,
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

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
