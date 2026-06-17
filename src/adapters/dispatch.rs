use crate::adapters::backend::factory::{BackendFactory, DefaultBackendFactory};
use crate::adapters::cli::Commands;
use crate::adapters::report_sink::JsonFileReportSink;
use crate::entities::common::{Error, Result};
use crate::entities::uri::Location;
use crate::usecases::config::validate_archive_template;
use crate::usecases::cull::CullReport;
use crate::usecases::move_text_shot::MoveTextShotReport;
use crate::usecases::report::{CopyReport, FindReport, Report, ReportSink};

/// 子命令执行结果：Copy/Move 返回 [`CopyReport`]，Find 返回 [`FindReport`]，
/// `MoveTextShot` 返回 [`MoveTextShotReport`]，`Cull` 返回 [`CullReport`]。
/// `tidy_with` 单一入口同时服务 CLI（丢弃返回）与 Android/mobile（消费 report）。
#[derive(Debug)]
pub enum CommandResult {
    Copy(CopyReport),
    Find(FindReport),
    MoveTextShot(MoveTextShotReport),
    Cull(CullReport),
}

/// 用默认 backend factory 跑命令；旧入口，等价于 `tidy_with(&DefaultBackendFactory, ...)`。
///
/// # Errors
///
/// 当命令执行过程中发生 IO 错误、backend 构造失败、业务逻辑出错，或 Copy/Move
/// 出现非零 failed（部分文件复制失败）时返回 `Err`，让 CLI 退出码非 0 让 CI/cron
/// 脚本能区分"全部成功"与"部分失败"。
pub fn tidy(command: Commands) -> Result<()> {
    let result = tidy_with(&DefaultBackendFactory, command)?;
    match result {
        CommandResult::Copy(report) if report.failed > 0 => {
            Err(Error::Io(std::io::Error::other(format!(
                "copy partial failure: {} failed, {} copied, {} ignored",
                report.failed, report.copied, report.ignored
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
        CommandResult::Copy(_)
        | CommandResult::Find(_)
        | CommandResult::MoveTextShot(_)
        | CommandResult::Cull(_) => Ok(()),
    }
}

/// 注入版入口：调用方提供 [`BackendFactory`]，常用于集成测试用 fake 装配混合 scheme。
/// 返回结构化 [`CommandResult`]：CLI 路径直接 `?` 丢弃，mobile 路径 match 取 report。
///
/// # Errors
///
/// 当 backend 构造失败、IO 操作出错或业务逻辑出错时返回 `Err`。
pub fn tidy_with(factory: &dyn BackendFactory, command: Commands) -> Result<CommandResult> {
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
            sources,
            output,
            dry_run,
            /* remove = */ false,
            include_non_media,
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
            sources,
            output,
            dry_run,
            /* remove = */ true,
            include_non_media,
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
        } => dispatch_move_text_shot(factory, sources, output, dry_run, report.as_deref()),
        Commands::Cull {
            dry_run,
            sources,
            output,
            phash_max,
            report,
        } => dispatch_cull(
            factory,
            sources,
            output,
            dry_run,
            phash_max,
            report.as_deref(),
        ),
    }
}

// Copy / Move 唯一区别是 `remove` 布尔；提到此处避免两个 arm 18 行同体重复。
#[expect(
    clippy::too_many_arguments,
    reason = "dispatch 单点接 6 个 CLI flag + factory + sources/output；折成结构体会让两个调用点也要先 Build 结构体"
)]
fn dispatch_copy_or_move(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    output: Location,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report: Option<&str>,
) -> Result<CommandResult> {
    validate_template_arg(archive_template)?;
    let src_pairs = build_sources(factory, sources)?;
    let out_pair = build_source(factory, output)?;
    let sink = report.map(JsonFileReportSink::new);
    let copy_report = crate::usecases::copy_with_sidecar(
        &src_pairs,
        out_pair,
        dry_run,
        remove,
        include_non_media,
        archive_template,
        sink.as_ref().map(|s| s as &dyn ReportSink),
        // P3 sidecar 发现的依赖倒置注入点：adapters 协议解析进 usecases 流程。
        Some(crate::adapters::sidecar::discover_with_backend),
    )?;
    Ok(CommandResult::Copy(copy_report))
}

fn dispatch_find(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    output: Option<Location>,
    secure: bool,
    report: Option<&str>,
) -> Result<CommandResult> {
    let src_pairs = build_sources(factory, sources)?;
    let out_pair = output.map(|loc| build_source(factory, loc)).transpose()?;
    let find_report = crate::usecases::find_duplicates(secure, src_pairs, out_pair.as_ref())?;
    // Find use case 当前不接 sink（report 由 dispatch 层捕获最终结构后落盘），
    // 与 Copy/Move 把 sink 当参数传给 use case 的形态不对称——find_duplicates
    // 无 progress 回调需求，单点写盘已够；若未来需要流式输出再改为同 Copy 形态。
    if let Some(path) = report {
        let sink = JsonFileReportSink::new(path);
        sink.write(&Report::Find(&find_report));
    }
    Ok(CommandResult::Find(find_report))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "由 Commands::MoveTextShot enum 解构 by-value 而来；usecase 接 &[]/& 借用"
)]
fn dispatch_move_text_shot(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    output: Location,
    dry_run: bool,
    report_path: Option<&str>,
) -> Result<CommandResult> {
    let ocr_cfg = &crate::usecases::config::config().backend.ocr;
    let detector = crate::adapters::ocr::build_detector(ocr_cfg)?;
    let move_report =
        crate::usecases::move_text_shot(detector.as_ref(), factory, &sources, &output, dry_run)?;
    if let Some(path) = report_path {
        let sink = JsonFileReportSink::new(path);
        sink.write(&Report::MoveTextShot(&move_report));
    }
    Ok(CommandResult::MoveTextShot(move_report))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "由 Commands::Cull enum 解构 by-value 而来；usecase 接 &[]/& 借用"
)]
fn dispatch_cull(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    output: Location,
    dry_run: bool,
    phash_max: Option<u8>,
    report_path: Option<&str>,
) -> Result<CommandResult> {
    let face_cfg = &crate::usecases::config::config().backend.face;
    let scrfd = crate::adapters::face::build_scrfd_detector(face_cfg)?;
    let facenet = crate::adapters::face::build_facenet_embedder(face_cfg)?;
    let facemesh = crate::adapters::face::build_facemesh(face_cfg)?;
    let eyestate = crate::adapters::face::build_eyestate_classifier(face_cfg)?;
    let cull_report = crate::usecases::cull(
        scrfd.as_ref(),
        facenet.as_ref(),
        facemesh.as_ref(),
        eyestate.as_ref(),
        factory,
        &sources,
        &output,
        dry_run,
        phash_max.unwrap_or(face_cfg.phash_hamming_max),
    )?;
    if let Some(path) = report_path {
        let sink = JsonFileReportSink::new(path);
        sink.write(&Report::Cull(&cull_report));
    }
    Ok(CommandResult::Cull(cull_report))
}

// None 表示未传，跳过校验；Some(s) 时校验模板合法性。
fn validate_template_arg(template: Option<&str>) -> Result<()> {
    let Some(t) = template else {
        return Ok(());
    };
    validate_archive_template(t).map_err(|msg| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid --archive-template: {msg}"),
        ))
    })
}

fn build_source(factory: &dyn BackendFactory, loc: Location) -> Result<crate::usecases::Source> {
    let backend = factory.for_location(&loc)?;
    Ok((loc, backend))
}

fn build_sources(
    factory: &dyn BackendFactory,
    locs: Vec<Location>,
) -> Result<Vec<crate::usecases::Source>> {
    locs.into_iter()
        .map(|loc| build_source(factory, loc))
        .collect()
}
