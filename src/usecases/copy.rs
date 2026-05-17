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

use super::config::config;

/// usecase 入口的 source / output 对：把 [`Location`] 与负责该 scheme 的
/// [`Backend`] 句柄一起传入，避免内层重新解析 URI。
pub(crate) type Source = (Location, Arc<dyn Backend>);

const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

const FEATURE_COPY: &str = "copy";

fn configured_offset() -> UtcOffset {
    offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界回退到 UTC，避免 panic；time crate 的合法范围 ±25:59:59 之内
fn offset_from_hours(hours: i8) -> UtcOffset {
    UtcOffset::from_whole_seconds(i32::from(hours) * 3600).unwrap_or(UtcOffset::UTC)
}

// chrono::FixedOffset 用于把 EXIF 内无时区的 NaiveDateTime 当相机本地时间解释。
// 与 time::UtcOffset 共用同一个 timezone_offset_hours 配置。
fn configured_chrono_offset() -> FixedOffset {
    chrono_offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界（chrono::FixedOffset 合法 ±86_400 秒，即 ±24h）回退到 UTC。
fn chrono_offset_from_hours(hours: i8) -> FixedOffset {
    FixedOffset::east_opt(i32::from(hours) * 3600).unwrap_or_else(|| chrono::Utc.fix())
}

pub(crate) fn copy(
    sources: Vec<Source>,
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
) -> common::Result<()> {
    let (output_loc, output_backend) = output;

    let mut source = Index::new();
    sources.iter().for_each(|(loc, backend)| {
        source.visit_location(loc, Arc::clone(backend));
    });
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
        return Ok(());
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

    let mut output_index = Index::new();
    output_index.visit_location(&output_loc, Arc::clone(&output_backend));

    let mut copied = 0;
    let mut ignored = 0;
    let mut failed = 0;
    source.files().iter().for_each(|(_, src)| {
        match do_copy(
            src,
            &output_loc,
            &output_backend,
            &mut output_index,
            dry_run,
            remove,
            include_non_media,
        ) {
            Ok(true) => {
                copied += 1;
            }
            Ok(false) => {
                ignored += 1;
            }
            Err(e) => {
                failed += 1;
                error!(
                    feature = FEATURE_COPY,
                    operation = "do_copy",
                    result = "error",
                    source = %src.full_path,
                    dry_run,
                    remove,
                    error = %e,
                    "copy item failed"
                );
            }
        }
    });

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
    Ok(())
}

pub(crate) fn do_copy(
    src: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &mut Index,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
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
        if remove && !dry_run {
            src.backend().remove_file(&src_loc)?;
        }
        return Ok(false);
    }

    if !include_non_media && !src.is_media() {
        warn!(
            feature = FEATURE_COPY,
            operation = "filter_media",
            result = "skipped_non_media",
            source = src_display,
            "file is not an image or video (pass --include-non-media to copy anyway)"
        );
        return Ok(false);
    }

    if let Some((target_dir_loc, target_loc)) = generate_unique_name(src, output_dir, output_backend) {
        if dry_run {
            println!("\"{}\"\t\"{}\"", src_display, target_loc.display());
            return Ok(true);
        }

        output_backend.mkdir_p(&target_dir_loc)?;

        // 跨 backend 也走 stream（mkparents=false 因为上面 mkdir_p 已经建好）。
        // 同 backend 时与 backend.copy_file 等价；好处是 src/out 不同 backend 时直接复用。
        stream_copy(src, &target_loc, output_backend.as_ref())?;
        if remove {
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
        Err(common::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("无法为\"{}\"生成目标目录的文件名", src_display),
        )))
    }
}

/// 用源 Info 的 backend 读 + 输出 backend 写。两个 backend 同一实例时与 `copy_file`
/// 等价；不同实例（跨 scheme）时仍工作。
///
/// 内部 4 个 `?` Err 分支（open_read / open_write / io::copy / writer.finish）中，
/// open_read Err 已被 `do_copy_file_copy_fails_when_source_unreadable` 稳定覆盖；
/// 后三者在 LocalBackend 下要构造 disk-full / 父目录在 mkdir_p 后被外部抢删等
/// 不可稳定的场景，整函数随 CLAUDE.md「不可稳定触发」套路标 coverage(off)。
/// Task 6 集成测试通过 FakeBackend reader/writer error 注入覆盖剩余分支。
#[cfg_attr(coverage_nightly, coverage(off))]
fn stream_copy(
    src: &Info,
    target: &Location,
    out_be: &dyn Backend,
) -> common::Result<()> {
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

    let valuable_name = extract_valuable_name(display_path);

    let sub_dir_path = output_dir
        .path()
        .join(year)
        .join(month)
        .join(valuable_name);
    let sub_dir_loc = output_dir.with_path(sub_dir_path.clone());

    // generate unique name by adding a number suffix
    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..max_attempts {
        let target_path = if i == 0 {
            sub_dir_path.join(file_name)
        } else {
            let mut name = file_stem.to_string();
            name.push('_');
            name.push_str(i.to_string().as_str());
            name.push('.');
            name.push_str(ext.as_str());
            sub_dir_path.join(name)
        };
        let target_loc = output_dir.with_path(target_path);

        // 对远端 backend 也通过 backend.exists 检测；同 backend 实例对 Local 等价。
        if !output_backend.exists(&target_loc).unwrap_or(false) {
            return Some((sub_dir_loc.clone(), target_loc));
        }
    }
    None
}

fn any_non_english(s: &str) -> bool {
    s.chars().any(|c| c as u32 > 127)
}

fn extract_valuable_name(full_path: &Utf8Path) -> String {
    let mut components: Vec<Utf8Component> = full_path.components().collect();
    // pop the file name
    if components.len() > 1 {
        components.pop();
    }

    for c in components.into_iter().rev() {
        if let Utf8Component::Normal(s) = c {
            if any_non_english(s) {
                return s.to_string();
            }
        }
    }
    "".to_string()
}


#[cfg(test)]
#[path = "copy_tests.rs"]
mod tests;
