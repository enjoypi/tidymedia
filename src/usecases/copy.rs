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
) -> common::Result<()> {
    let mut source = Index::new();
    input_dirs.iter().for_each(|s| {
        source.visit_dir(s.as_str());
    });
    source.parse_exif()?;

    let total_files = source.files().len();
    info!(
        feature = FEATURE_COPY,
        operation = "scan_sources",
        result = "ok",
        total_files,
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
        match do_copy(src, &output_path, &mut output_index, dry_run, remove) {
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

    info!(
        feature = FEATURE_COPY,
        operation = "summary",
        result = if failed == 0 { "ok" } else { "partial" },
        total = total_files,
        copied,
        ignored,
        failed,
        dry_run,
        remove,
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
) -> common::Result<bool> {
    let full_path = src.full_path.as_str();

    if let Some(dup) = output_index.exists(src)? {
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

    if !src.is_media() {
        warn!(
            feature = FEATURE_COPY,
            operation = "filter_media",
            result = "skipped_non_media",
            source = full_path,
            "file is not an image or video"
        );
        return Ok(false);
    }

    if let Some((target_dir, target)) = generate_unique_name(src, output_dir)? {
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

        _ = output_index.add(Info::from(target)?);

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
) -> common::Result<Option<(String, String)>> {
    let full_path = Utf8Path::new(src_file.full_path.as_str());
    let file_name = full_path
        .file_name()
        .expect("Info::from guarantees file path has a name");
    let file_stem = full_path
        .file_stem()
        .expect("file with name must have a stem")
        .to_string();
    let ext = full_path.extension().unwrap_or("").to_string();

    let create_time = src_file.create_time(config().exif.valid_date_time_secs)?;
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

            return Ok(Some((sub_dir, target)));
        }
    }
    Ok(None)
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
mod test {
    use super::*;

    #[test]
    fn test_match_non_english() {
        assert!(!any_non_english("abc"));
        assert!(any_non_english("abc中文"));
    }

    #[test]
    fn extract_valuable_name_finds_last_non_english_dir() {
        let path = Utf8Path::new(
            "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/中文/abc",
        );
        assert_eq!(extract_valuable_name(path), "中文");
    }

    #[test]
    fn extract_valuable_name_returns_empty_when_all_ascii() {
        let path = Utf8Path::new("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z");
        assert_eq!(extract_valuable_name(path), "");
    }

    #[test]
    fn extract_valuable_name_root_returns_empty() {
        let path = Utf8Path::new("/");
        assert_eq!(extract_valuable_name(path), "");
    }

    #[test]
    fn extract_valuable_name_single_component_returns_empty() {
        let path = Utf8Path::new("photo");
        assert_eq!(extract_valuable_name(path), "");
    }

    #[test]
    fn extract_valuable_name_picks_innermost_non_english() {
        let path = Utf8Path::new("/外层/内层/file.png");
        assert_eq!(extract_valuable_name(path), "内层");
    }

    #[test]
    fn extract_valuable_name_handles_mixed_chars() {
        let path = Utf8Path::new("/p/a高一 元旦晚会/sub/p.png");
        assert_eq!(extract_valuable_name(path), "a高一 元旦晚会");
    }

    #[test]
    fn offset_from_hours_valid_value_produces_expected_offset() {
        let off = offset_from_hours(8);
        assert_eq!(off.whole_hours(), 8);
    }

    #[test]
    fn offset_from_hours_out_of_range_falls_back_to_utc() {
        // 127*3600 秒远超 time crate ±25:59:59 范围，应回退 UTC
        let off = offset_from_hours(127);
        assert_eq!(off, UtcOffset::UTC);
    }
}

#[cfg(test)]
mod test_io {
    use std::fs;
    use std::path::Path;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use crate::entities::test_common as tc;
    use super::*;

    fn utf8(p: &Path) -> Utf8PathBuf {
        Utf8PathBuf::from(p.to_str().unwrap())
    }

    fn make_media_info(dir: &Path, name: &str) -> Info {
        let png = tc::copy_png_to(dir, name).unwrap();
        let mut info = Info::from(png.to_str().unwrap()).unwrap();
        let exif: crate::entities::exif::Exif = serde_json::from_value(
            serde_json::json!({
                "SourceFile": info.full_path.as_str().to_string(),
                "File:MIMEType": "image/png",
            }),
        )
        .unwrap();
        info.set_exif(exif);
        info
    }

    fn fill_collisions(sub: &Path) {
        fs::create_dir_all(sub).unwrap();
        fs::write(sub.join("photo.png"), b"").unwrap();
        for i in 1..10 {
            fs::write(sub.join(format!("photo_{i}.png")), b"").unwrap();
        }
    }

    #[test]
    fn copy_empty_source_returns_ok() {
        let src = tempdir().unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn copy_dry_run_does_not_write() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), true, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn copy_writes_into_year_month_valuable_path() {
        let src = tempdir().unwrap();
        let nested = src.path().join("假日相册");
        fs::create_dir_all(&nested).unwrap();
        tc::copy_png_to(&nested, "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false).unwrap();
        let expected = out
            .path()
            .join("2024")
            .join("01")
            .join("假日相册")
            .join("photo.png");
        assert!(expected.exists(), "expected file at {expected:?}");
    }

    #[test]
    fn copy_skips_duplicate_already_in_output() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        fs::copy(tc::DATA_DNS_BENCHMARK, out.path().join("already.png")).unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
    }

    #[test]
    fn move_removes_source_when_duplicate_exists() {
        let src = tempdir().unwrap();
        let png_src = tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        fs::copy(tc::DATA_DNS_BENCHMARK, out.path().join("already.png")).unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, true).unwrap();
        assert!(!png_src.exists(), "source duplicate should be removed");
    }

    #[test]
    fn move_renames_into_output() {
        let root = tempdir().unwrap();
        let src_dir = root.path().join("src");
        let out_dir = root.path().join("out");
        fs::create_dir_all(&src_dir).unwrap();
        let png_src = tc::copy_png_to(&src_dir, "photo.png").unwrap();
        copy(vec![utf8(&src_dir)], utf8(&out_dir), false, true).unwrap();
        assert!(!png_src.exists());
        let expected = out_dir.join("2024").join("01").join("photo.png");
        assert!(expected.exists(), "expected moved file at {expected:?}");
    }

    #[test]
    fn do_copy_skips_non_media_files() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("plain.bin"), b"abc").unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn generate_unique_name_uses_suffix_when_first_taken() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        let out_utf8 = utf8(out.path());
        let sub = out.path().join("2024").join("01");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("photo.png"), b"x").unwrap();
        let (_, target) = generate_unique_name(&info, &out_utf8)
            .unwrap()
            .expect("unique name should be generated");
        assert!(target.ends_with("photo_1.png"), "got {target}");
    }

    #[test]
    fn generate_unique_name_none_after_10_collisions() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        fill_collisions(&out.path().join("2024").join("01"));
        let res = generate_unique_name(&info, &utf8(out.path())).unwrap();
        assert!(res.is_none(), "should exhaust after 10 collisions");
    }

    #[test]
    fn do_copy_errors_when_unique_name_exhausted() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        fill_collisions(&out.path().join("2024").join("01"));
        let mut idx = crate::entities::file_index::Index::new();
        let err = do_copy(&info, &utf8(out.path()), &mut idx, false, false)
            .expect_err("must error after collisions");
        assert!(err.to_string().contains("无法为"));
    }

    #[test]
    fn copy_logs_failure_when_target_collisions_exhausted() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        fill_collisions(&out.path().join("2024").join("01"));
        copy(vec![utf8(src.path())], utf8(out.path()), false, false).unwrap();
    }

    #[test]
    fn do_copy_dry_run_reports_target_but_writes_nothing() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let did_copy = do_copy(&info, &utf8(out.path()), &mut idx, true, false).unwrap();
        assert!(did_copy);
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }
}
