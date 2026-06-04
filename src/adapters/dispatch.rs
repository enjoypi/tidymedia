use camino::Utf8PathBuf;

use crate::adapters::backend::factory::{BackendFactory, DefaultBackendFactory};
use crate::adapters::cli::Commands;
use crate::entities::common::{Error, Result};
use crate::entities::uri::Location;
use crate::usecases::config::validate_archive_template;
#[cfg(feature = "android-app")]
use crate::usecases::report::CopyReport;
use crate::usecases::report::{FindReport, write_find_report};

/// 用默认 backend factory 跑命令；旧入口，等价于 `tidy_with(&DefaultBackendFactory, ...)`。
///
/// # Errors
///
/// 当命令执行过程中发生 IO 错误、backend 构造失败或业务逻辑出错时返回 `Err`。
pub fn tidy(command: Commands) -> Result<()> {
    tidy_with(&DefaultBackendFactory, command)
}

/// 注入版入口：调用方提供 [`BackendFactory`]，常用于集成测试用 fake 装配混合 scheme。
///
/// # Errors
///
/// 当 backend 构造失败、IO 操作出错或业务逻辑出错时返回 `Err`。
pub fn tidy_with(factory: &dyn BackendFactory, command: Commands) -> Result<()> {
    match command {
        Commands::Copy {
            dry_run,
            include_non_media,
            sources,
            output,
            archive_template,
            report,
        } => {
            validate_template_arg(archive_template.as_deref())?;
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = build_source(factory, output)?;
            crate::usecases::copy(
                &src_pairs,
                out_pair,
                dry_run,
                false,
                include_non_media,
                archive_template.as_deref(),
                report.as_deref(),
            )
            .map(|_| ())
        }
        Commands::Find {
            secure,
            sources,
            output,
            report,
        } => {
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = output.map(|loc| build_source(factory, loc)).transpose()?;
            let groups = crate::usecases::find_duplicates(secure, src_pairs, out_pair.as_ref());
            if let Some(path) = report.as_deref() {
                // scanned = total file paths across all groups（每组均为重复集，无 singleton）
                let scanned = groups.values().flatten().count();
                let report_data = FindReport {
                    scanned,
                    groups: groups
                        .into_values()
                        .map(|paths| {
                            paths
                                .into_iter()
                                .map(|p: Utf8PathBuf| p.to_string())
                                .collect()
                        })
                        .collect(),
                    // bytes_read 在 find_duplicates 内部；此处仅统计重复组内路径数作轻量摘要。
                    bytes_read: 0,
                };
                write_find_report(path, &report_data);
            }
            Ok(())
        }
        Commands::Move {
            dry_run,
            include_non_media,
            sources,
            output,
            archive_template,
            report,
        } => {
            validate_template_arg(archive_template.as_deref())?;
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = build_source(factory, output)?;
            crate::usecases::copy(
                &src_pairs,
                out_pair,
                dry_run,
                true,
                include_non_media,
                archive_template.as_deref(),
                report.as_deref(),
            )
            .map(|_| ())
        }
    }
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

/// copy コマンドを実行して [`CopyReport`] を返す Android / mobile 専用入口。
/// `tidy_with` は `Result<()>` で report を捨てるため、mobile 層が stats を取り出せない。
/// このラッパーは Copy / Move 以外のコマンドを受け取ると `InvalidInput` を返す。
///
/// # Errors
///
/// backend 構築失敗、IO エラー、または非 Copy/Move コマンド時に `Err`。
#[cfg(feature = "android-app")]
pub(crate) fn copy_report(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    output: Location,
    dry_run: bool,
) -> Result<CopyReport> {
    let src_pairs = build_sources(factory, sources)?;
    let out_pair = build_source(factory, output)?;
    crate::usecases::copy(&src_pairs, out_pair, dry_run, false, false, None, None)
}

/// find コマンドを実行して重複グループを [`FindReport`] として返す Android / mobile 専用入口。
///
/// # Errors
///
/// backend 構築失敗または IO エラー時に `Err`。
#[cfg(feature = "android-app")]
pub(crate) fn find_report(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    secure: bool,
) -> Result<FindReport> {
    let src_pairs = build_sources(factory, sources)?;
    let groups = crate::usecases::find_duplicates(secure, src_pairs, None);
    let scanned = groups.values().flatten().count();
    let report = FindReport {
        scanned,
        groups: groups
            .into_values()
            .map(|paths| {
                paths
                    .into_iter()
                    .map(|p: Utf8PathBuf| p.to_string())
                    .collect()
            })
            .collect(),
        bytes_read: 0,
    };
    Ok(report)
}
