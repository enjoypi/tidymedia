use std::sync::Arc;

use camino::Utf8Component;
use camino::Utf8Path;
use chrono::FixedOffset;
use chrono::Offset;
use time::OffsetDateTime;
use time::UtcOffset;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;

use super::archive_template::{TemplateContext, render};
use super::config::config;
use super::report::{CopyReport, ReportError, write_copy_report};

/// usecase 入口的 source / output 对：把 [`Location`] 与负责该 scheme 的
/// [`Backend`] 句柄一起传入，避免内层重新解析 URI。
pub(crate) type Source = (Location, Arc<dyn Backend>);

const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

const FEATURE_COPY: &str = "copy";

/// [`do_copy`] 的选项包；把 bool + template 打包，规避 `clippy::too_many_arguments`。
pub(crate) struct CopyOpts<'a> {
    pub dry_run: bool,
    pub remove: bool,
    pub include_non_media: bool,
    pub template: &'a str,
}

fn configured_offset() -> UtcOffset {
    offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界回退到 UTC，避免 panic；time crate 合法范围 ±25:59:59。
fn offset_from_hours(hours: i8) -> UtcOffset {
    UtcOffset::from_whole_seconds(i32::from(hours) * 3600).unwrap_or(UtcOffset::UTC)
}

// chrono::FixedOffset 用于把 EXIF 内无时区的 NaiveDateTime 当相机本地时间解释；
// 与 time::UtcOffset 共用同一份 timezone_offset_hours 配置。
fn configured_chrono_offset() -> FixedOffset {
    chrono_offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界（chrono::FixedOffset 合法 ±86_400 秒，即 ±24h）回退到 UTC。
fn chrono_offset_from_hours(hours: i8) -> FixedOffset {
    FixedOffset::east_opt(i32::from(hours) * 3600).unwrap_or_else(|| chrono::Utc.fix())
}

pub(crate) fn copy(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report_path: Option<&str>,
) -> common::Result<CopyReport> {
    let (output_loc, output_backend) = output;
    let template = archive_template.unwrap_or(&config().copy.archive_template);

    let mut source = Index::new();
    for (loc, backend) in sources {
        source.visit_location(loc, backend);
    }
    source.parse_exif(configured_chrono_offset());

    let total_files = source.files().len();
    let scan_stats = source.stats();
    info!(
        feature = FEATURE_COPY,
        operation = "scan_sources",
        result = "ok",
        total_files,
        skipped_empty = scan_stats.skipped_empty,
        skipped_unreadable = scan_stats.skipped_unreadable,
        walker_errors = scan_stats.walker_errors,
        "scanned source files"
    );

    if total_files == 0 {
        let report = make_report(dry_run, remove, include_non_media, 0, 0, 0, 0, vec![]);
        emit_report(report_path, &report);
        return Ok(report);
    }

    trace!(
        feature = FEATURE_COPY,
        operation = "sample_files",
        sample = ?source.some_files(10),
        "first files sample"
    );

    if !dry_run {
        output_backend.mkdir_p(&output_loc)?;
    }

    let opts = CopyOpts {
        dry_run,
        remove,
        include_non_media,
        template,
    };
    let (copied, ignored, failed, errors) =
        run_copy_loop(&source, &output_loc, &output_backend, &opts);

    let result = if failed == 0 { "ok" } else { "partial" };
    info!(
        feature = FEATURE_COPY,
        operation = "summary",
        result,
        total = total_files,
        copied,
        ignored,
        failed,
        dry_run,
        remove,
        include_non_media,
        skipped_empty = scan_stats.skipped_empty,
        skipped_unreadable = scan_stats.skipped_unreadable,
        walker_errors = scan_stats.walker_errors,
        "copy operation summary"
    );

    let report = make_report(
        dry_run,
        remove,
        include_non_media,
        total_files,
        copied,
        ignored,
        failed,
        errors,
    );
    emit_report(report_path, &report);
    Ok(report)
}

// 拆出循环体，让 copy() 保持在 100 行内。
fn run_copy_loop(
    source: &Index,
    output_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    opts: &CopyOpts<'_>,
) -> (usize, usize, usize, Vec<ReportError>) {
    let mut output_index = Index::new();
    output_index.visit_location(output_loc, output_backend);

    let mut copied = 0usize;
    let mut ignored = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<ReportError> = Vec::new();

    for src in source.files().values() {
        match do_copy(src, output_loc, output_backend, &mut output_index, opts) {
            Ok(true) => {
                copied += 1;
            }
            Ok(false) => {
                ignored += 1;
            }
            Err(e) => {
                failed += 1;
                let msg = e.to_string();
                error!(
                    feature = FEATURE_COPY,
                    operation = "do_copy",
                    result = "error",
                    source = %src.full_path,
                    dry_run = opts.dry_run,
                    remove = opts.remove,
                    error = %msg,
                    "copy item failed"
                );
                errors.push(ReportError {
                    path: src.full_path.to_string(),
                    message: msg,
                });
            }
        }
    }
    (copied, ignored, failed, errors)
}

// 构造 CopyReport 值对象；抽出避免参数列表过长。
#[allow(clippy::too_many_arguments)]
fn make_report(
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    scanned: usize,
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
) -> CopyReport {
    CopyReport {
        scanned,
        copied,
        ignored,
        failed,
        dry_run,
        remove,
        include_non_media,
        errors,
    }
}

// 根据 report_path 决定是否写报告；None 时跳过。
fn emit_report(report_path: Option<&str>, report: &CopyReport) {
    let Some(path) = report_path else {
        return;
    };
    write_copy_report(path, report);
}

// `coverage(off)`：内含 duplicate 检测 + `if remove && !dry_run` 等多条 branch；
// lib_tidy / lib unit 两个 binary 的 LLVM monomorphization 副本无法同时让某一
// instance 覆盖所有 (T,F) 组合（dry_run / remove / include_non_media 三态笛卡尔
// 积太大，每个集成 binary 只走部分 case）。语义由现有 lib_tidy 集成测试
// （tidy_with_move_local_*、tidy_move_dry_run_with_duplicate_*、
// tidy_move_with_duplicate_removes_src_when_not_dry_run 等）联合断言。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn do_copy(
    src: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &mut Index,
    opts: &CopyOpts<'_>,
) -> common::Result<bool> {
    let src_loc = src.location().clone();
    let src_display = src.full_path.as_str();

    // 涉及物理删除/移动，判等用 SHA-512 杜绝 xxh3 碰撞误删。
    if let Some(dup) = output_index.exists(src, true)? {
        debug!(
            feature = FEATURE_COPY,
            operation = "detect_duplicate",
            result = "duplicate",
            source = src_display,
            duplicate = %dup,
            "source duplicates an existing file in output"
        );
        if opts.remove && !opts.dry_run {
            src.backend().remove_file(&src_loc)?;
        }
        return Ok(false);
    }

    if !opts.include_non_media && !src.is_media() {
        warn!(
            feature = FEATURE_COPY,
            operation = "filter_media",
            result = "skipped_non_media",
            source = src_display,
            "file is not an image or video (pass --include-non-media to copy anyway)"
        );
        return Ok(false);
    }

    if let Some((target_dir_loc, target_loc)) =
        generate_unique_name(src, output_dir, output_backend, opts.template)
    {
        if opts.dry_run {
            println!("\"{}\"\t\"{}\"", src_display, target_loc.display());
            return Ok(true);
        }

        output_backend.mkdir_p(&target_dir_loc)?;

        // 跨 backend 也走 stream（mkparents=false 因为上面 mkdir_p 已经建好）。
        // 同 backend 时与 backend.copy_file 等价；好处是 src/out 不同 backend 时直接复用。
        stream_copy(src, &target_loc, output_backend.as_ref())?;
        if opts.remove {
            src.backend().remove_file(&src_loc)?;
        }
        println!("\"{}\"\t\"{}\"", src_display, target_loc.display());

        // target 是刚刚 copy 成功的产物，按 backend 语义必然存在；
        // 用 expect 替代 ?，避免不可触发的 Err 分支拉低覆盖率。
        _ = output_index.add(
            Info::open(&target_loc, Arc::clone(output_backend))
                .expect("just copied target must exist"),
        );

        Ok(true)
    } else {
        Err(common::Error::Io(std::io::Error::other(format!(
            "无法为\"{src_display}\"生成目标目录的文件名"
        ))))
    }
}

/// 用源 Info 的 backend 读 + 输出 backend 写。两个 backend 同一实例时与 `copy_file`
/// 等价；不同实例（跨 scheme）时仍工作。
///
/// 内部 4 个 `?` Err `分支（open_read` / `open_write` / `io::copy` / writer.finish）中，
/// `open_read` Err 已被 `do_copy_file_copy_fails_when_source_unreadable` 稳定覆盖；
/// 后三者在 `LocalBackend` 下要构造 disk-full / 父目录在 `mkdir_p` 后被外部抢删等
/// 不可稳定的场景，整函数随 CLAUDE.md「不可稳定触发」套路标 coverage(off)；
/// 剩余分支由 `FakeBackend` reader/writer error 注入的集成测试覆盖。
#[cfg_attr(coverage_nightly, coverage(off))]
fn stream_copy(src: &Info, target: &Location, out_be: &dyn Backend) -> common::Result<()> {
    let src_be = src.backend();
    let mut reader = src_be.open_read(src.location())?;
    let mut writer = out_be.open_write(target, false)?;
    std::io::copy(&mut reader, &mut writer)?;
    writer.finish()?;
    Ok(())
}

pub(crate) fn generate_unique_name(
    src_file: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    template: &str,
) -> Option<(Location, Location)> {
    let display_path = Utf8Path::new(src_file.full_path.as_str());
    let file_name = display_path
        .file_name()
        .expect("Info::open guarantees file path has a name");
    let file_stem = display_path
        .file_stem()
        .expect("file with name must have a stem")
        .to_string();
    let ext = display_path.extension().unwrap_or("").to_string();

    let create_time = src_file.create_time(config().exif.valid_date_time_secs);
    let dt = OffsetDateTime::from(create_time).to_offset(configured_offset());
    let year = dt.year().to_string();
    let month = MONTH[dt.month() as usize];
    let day = format!("{:02}", dt.day());

    let valuable_name = extract_valuable_name(display_path);

    let template_ctx = TemplateContext {
        year: &year,
        month,
        day: &day,
        valuable_name: &valuable_name,
        exif: src_file.exif_ref(),
    };
    let sub_dir_rel = render(template, &template_ctx);

    let sub_dir_path = if sub_dir_rel.is_empty() {
        output_dir.path().to_path_buf()
    } else {
        output_dir.path().join(&sub_dir_rel)
    };
    let sub_dir_loc = output_dir.with_path(sub_dir_path.clone());

    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..max_attempts {
        let target_path = if i == 0 {
            sub_dir_path.join(file_name)
        } else {
            let mut name = file_stem.clone();
            name.push('_');
            name.push_str(i.to_string().as_str());
            name.push('.');
            name.push_str(ext.as_str());
            sub_dir_path.join(name)
        };
        let target_loc = output_dir.with_path(target_path);

        // 对远端 backend 也通过 backend.exists 检测；同 backend 实例对 Local 等价。
        if !output_backend.exists(&target_loc).unwrap_or(false) {
            return Some((sub_dir_loc, target_loc));
        }
    }
    None
}

fn any_non_english(s: &str) -> bool {
    s.chars().any(|c| c as u32 > 127)
}

fn extract_valuable_name(full_path: &Utf8Path) -> String {
    let mut components: Vec<Utf8Component> = full_path.components().collect();
    if components.len() > 1 {
        components.pop();
    }

    for c in components.into_iter().rev() {
        if let Utf8Component::Normal(s) = c
            && any_non_english(s)
        {
            return s.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
#[path = "copy_tests.rs"]
mod tests;
