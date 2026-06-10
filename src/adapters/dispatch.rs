use crate::adapters::backend::factory::{BackendFactory, DefaultBackendFactory};
use crate::adapters::cli::Commands;
use crate::adapters::report_sink::JsonFileReportSink;
use crate::entities::common::{Error, Result};
use crate::entities::uri::Location;
use crate::usecases::config::validate_archive_template;
use crate::usecases::report::{CopyReport, FindReport, Report, ReportSink};

/// 子命令执行结果：Copy/Move 返回 [`CopyReport`]，Find 返回 [`FindReport`]。
/// 让 `tidy_with` 单一入口同时服务 CLI（丢弃返回）与 Android/mobile（消费 report 字段）。
#[derive(Debug)]
pub enum CommandResult {
    Copy(CopyReport),
    Find(FindReport),
}

/// 用默认 backend factory 跑命令；旧入口，等价于 `tidy_with(&DefaultBackendFactory, ...)`。
///
/// # Errors
///
/// 当命令执行过程中发生 IO 错误、backend 构造失败或业务逻辑出错时返回 `Err`。
pub fn tidy(command: Commands) -> Result<()> {
    tidy_with(&DefaultBackendFactory, command).map(|_| ())
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
