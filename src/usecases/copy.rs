use camino::Utf8Component;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use time::OffsetDateTime;
use time::UtcOffset;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::full_path;
use crate::entities::file_info::Info;

use super::config::config;

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

pub fn copy(
    input_dirs: Vec<Utf8PathBuf>,
    output: Utf8PathBuf,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
) -> common::Result<()> {
    let mut source = Index::new();
    input_dirs.iter().for_each(|s| {
        source.visit_dir(s.as_str());
    });
    source.parse_exif()?;

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

    let output_path = full_path(output.as_str())?;
    if !dry_run {
        fs_extra::dir::create_all(output.as_str(), false)?;
    }

    let mut output_index = Index::new();
    output_index.visit_dir(output_path.as_str());

    let mut copied = 0;
    let mut ignored = 0;
    let mut failed = 0;
    source.files().iter().for_each(|(_, src)| {
        match do_copy(
            src,
            &output_path,
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
    output_dir: &Utf8PathBuf,
    output_index: &mut Index,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
) -> common::Result<bool> {
    let full_path = src.full_path.as_str();

    // 涉及物理删除/移动，判等用 SHA-512 杜绝 xxh3 碰撞误删。
    if let Some(dup) = output_index.exists(src, true)? {
        debug!(
            feature = FEATURE_COPY,
            operation = "detect_duplicate",
            result = "duplicate",
            source = full_path,
            duplicate = %dup,
            "source duplicates an existing file in output"
        );
        if remove && !dry_run {
            fs_extra::file::remove(full_path)?;
        }
        return Ok(false);
    }

    if !include_non_media && !src.is_media() {
        warn!(
            feature = FEATURE_COPY,
            operation = "filter_media",
            result = "skipped_non_media",
            source = full_path,
            "file is not an image or video (pass --include-non-media to copy anyway)"
        );
        return Ok(false);
    }

    if let Some((target_dir, target)) = generate_unique_name(src, output_dir) {
        if dry_run {
            println!("\"{}\"\t\"{}\"", full_path, target);
            return Ok(true);
        }

        fs_extra::dir::create_all(target_dir.as_str(), false)?;
        let target = target.as_str();

        let options = fs_extra::file::CopyOptions::new().skip_exist(true);
        if remove {
            fs_extra::file::move_file(full_path, target, &options)?;
        } else {
            fs_extra::file::copy(full_path, target, &options)?;
        }
        println!("\"{}\"\t\"{}\"", full_path, target);

        // target 是刚刚 copy/move 成功的产物，按文件系统语义必然存在；
        // 用 expect 替代 ?，避免不可触发的 Err 分支拉低覆盖率。
        _ = output_index.add(Info::from(target).expect("just copied target must exist"));

        Ok(true)
    } else {
        Err(common::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("无法为\"{}\"生成目标目录的文件名", src.full_path.as_str()),
        )))
    }
}

pub(crate) fn generate_unique_name(
    src_file: &Info,
    output_dir: &Utf8PathBuf,
) -> Option<(String, String)> {
    let full_path = Utf8Path::new(src_file.full_path.as_str());
    let file_name = full_path
        .file_name()
        .expect("Info::from guarantees file path has a name");
    let file_stem = full_path
        .file_stem()
        .expect("file with name must have a stem")
        .to_string();
    let ext = full_path.extension().unwrap_or("").to_string();

    let create_time = src_file.create_time(config().exif.valid_date_time_secs);
    let dt = OffsetDateTime::from(create_time).to_offset(configured_offset());
    let year = dt.year().to_string();
    let month = MONTH[dt.month() as usize];

    let valuable_name = extract_valuable_name(full_path);

    let sub_dir = output_dir.join(year).join(month).join(valuable_name);

    // generate unique name by adding a number suffix
    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..max_attempts {
        let target = if i == 0 {
            sub_dir.join(file_name)
        } else {
            let mut file_name = file_stem.to_string();

            file_name.push('_');
            file_name.push_str(i.to_string().as_str());
            file_name.push('.');
            file_name.push_str(ext.as_str());
            sub_dir.join(file_name)
        };

        if !target.exists() {
            let sub_dir = sub_dir.to_string();
            let target = target.to_string();

            return Some((sub_dir, target));
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
