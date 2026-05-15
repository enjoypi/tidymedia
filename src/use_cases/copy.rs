use camino::Utf8Component;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use time::OffsetDateTime;
use time::UtcOffset;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

use super::entities::common;
use super::entities::file_index::Index;
use super::entities::file_info::full_path;
use super::entities::file_info::Info;

const CST: std::result::Result<UtcOffset, time::error::ComponentRange> =
    UtcOffset::from_hms(8, 0, 0);
const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

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

    info!("源目录中有文件 {} 个", source.files().len());

    if source.files().is_empty() {
        return Ok(());
    }

    trace!("Files: {:#?}", source.some_files(10));

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
                error!("{}", e);
            }
        }
    });

    info!(
        "共 {} 个文件，复制了 {} 个文件，忽略了 {} 个文件，失败了 {} 个文件",
        source.files().len(),
        copied,
        ignored,
        failed
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
        trace!("\"{}\"\t和\t\"{}\"\t相同", full_path, dup);
        if remove && !dry_run {
            fs_extra::file::remove(full_path)?;
        }
        return Ok(false);
    }

    if !src.is_media() {
        warn!("\"{}\"\t不是图片或者视频文件", full_path);
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
            // 先尝试rename，同一个磁盘下相当于直接移动
            if std::fs::rename(full_path, target).is_err() {
                if let Err(e) = fs_extra::file::move_file(full_path, target, &options) {
                    error!("{}: \"{}\"\t移动失败\t\"{}\"", e, full_path, target);
                    return Ok(false);
                }
                println!("\"{}\"\t\"{}\"", full_path, target);
            }
        } else {
            if fs_extra::file::copy(full_path, target, &options)? != src.size {
                error!("\"{}\"\t复制失败\t\"{}\"", full_path, target);
                return Ok(false);
            }
            println!("\"{}\"\t\"{}\"", full_path, target);
        }

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
    let file_name = full_path.file_name().ok_or(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "Invalid file name",
    ))?;

    let file_stem = full_path.file_stem().unwrap().to_string();
    let ext = if full_path.extension().is_none() {
        "".to_string()
    } else {
        full_path.extension().unwrap().to_string()
    };

    let create_time = src_file.create_time()?;
    let dt = OffsetDateTime::from(create_time).to_offset(CST.expect("CST"));
    let year = dt.year().to_string();
    let month = MONTH[dt.month() as usize];

    let valuable_name = extract_valuable_name(full_path);

    let sub_dir = output_dir.join(year).join(month).join(valuable_name);

    // generate unique name by adding a number suffix
    for i in 0..10 {
        let target = if i <= 0 {
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
}

#[cfg(test)]
mod test_io {
    use std::fs;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::super::entities::test_common as tc;
    use super::*;

    #[test]
    fn copy_empty_source_returns_ok() -> tc::Result {
        let src = tempdir()?;
        let out = tempdir()?;
        copy(
            vec![Utf8PathBuf::from(src.path().to_str().unwrap())],
            Utf8PathBuf::from(out.path().to_str().unwrap()),
            false,
            false,
        )?;
        let entries: Vec<_> = fs::read_dir(out.path())?.collect();
        assert!(entries.is_empty());
        Ok(())
    }

    #[test]
    fn copy_dry_run_does_not_write() -> tc::Result {
        let src = tempdir()?;
        tc::copy_png_to(src.path(), "photo.png")?;
        let out = tempdir()?;
        copy(
            vec![Utf8PathBuf::from(src.path().to_str().unwrap())],
            Utf8PathBuf::from(out.path().to_str().unwrap()),
            true,
            false,
        )?;
        let entries: Vec<_> = fs::read_dir(out.path())?.collect();
        assert!(entries.is_empty(), "dry_run must not write");
        Ok(())
    }

    #[test]
    fn copy_actually_writes_files_into_year_month_path() -> tc::Result {
        let src = tempdir()?;
        let nested = src.path().join("假日相册");
        fs::create_dir_all(&nested)?;
        tc::copy_png_to(&nested, "photo.png")?;

        let out = tempdir()?;
        copy(
            vec![Utf8PathBuf::from(src.path().to_str().unwrap())],
            Utf8PathBuf::from(out.path().to_str().unwrap()),
            false,
            false,
        )?;

        let expected = out
            .path()
            .join("2024")
            .join("01")
            .join("假日相册")
            .join("photo.png");
        assert!(expected.exists(), "expected file at {expected:?}");
        Ok(())
    }

    #[test]
    fn copy_skips_duplicate_already_in_output() -> tc::Result {
        let src = tempdir()?;
        tc::copy_png_to(src.path(), "photo.png")?;

        let out = tempdir()?;
        let pre_target = out.path().join("already.png");
        fs::copy(tc::DATA_DNS_BENCHMARK, &pre_target)?;

        copy(
            vec![Utf8PathBuf::from(src.path().to_str().unwrap())],
            Utf8PathBuf::from(out.path().to_str().unwrap()),
            false,
            false,
        )?;

        let count = fs::read_dir(out.path())?.count();
        assert_eq!(count, 1, "output dir should still contain only the prepopulated copy");
        Ok(())
    }

    #[test]
    fn move_removes_source_when_duplicate_exists() -> tc::Result {
        let src = tempdir()?;
        let png_src = tc::copy_png_to(src.path(), "photo.png")?;

        let out = tempdir()?;
        let pre = out.path().join("already.png");
        fs::copy(tc::DATA_DNS_BENCHMARK, &pre)?;

        copy(
            vec![Utf8PathBuf::from(src.path().to_str().unwrap())],
            Utf8PathBuf::from(out.path().to_str().unwrap()),
            false,
            true,
        )?;

        assert!(!png_src.exists(), "source duplicate should be removed in move");
        Ok(())
    }

    #[test]
    fn move_renames_into_output() -> tc::Result {
        let common_root = tempdir()?;
        let src_dir = common_root.path().join("src");
        let out_dir = common_root.path().join("out");
        fs::create_dir_all(&src_dir)?;
        let png_src = tc::copy_png_to(&src_dir, "photo.png")?;

        copy(
            vec![Utf8PathBuf::from(src_dir.to_str().unwrap())],
            Utf8PathBuf::from(out_dir.to_str().unwrap()),
            false,
            true,
        )?;

        assert!(!png_src.exists(), "source should be moved away");
        let expected = out_dir.join("2024").join("01").join("photo.png");
        assert!(expected.exists(), "expected moved file at {expected:?}");
        Ok(())
    }

    #[test]
    fn do_copy_skips_non_media_files() -> tc::Result {
        let src = tempdir()?;
        fs::write(src.path().join("plain.bin"), b"abc")?;

        let out = tempdir()?;
        copy(
            vec![Utf8PathBuf::from(src.path().to_str().unwrap())],
            Utf8PathBuf::from(out.path().to_str().unwrap()),
            false,
            false,
        )?;

        let entries: Vec<_> = fs::read_dir(out.path())?.collect();
        assert!(entries.is_empty(), "non-media must be skipped");
        Ok(())
    }

    fn make_media_info(dir: &std::path::Path, name: &str) -> Result<Info, tc::Error> {
        let png = tc::copy_png_to(dir, name)?;
        let mut info = Info::from(png.to_str().unwrap())?;
        let exif: super::super::entities::exif::Exif = serde_json::from_value(
            serde_json::json!({
                "SourceFile": info.full_path.as_str().to_string(),
                "File:MIMEType": "image/png",
            }),
        )?;
        info.set_exif(exif);
        Ok(info)
    }

    #[test]
    fn generate_unique_name_uses_suffix_when_first_taken() -> tc::Result {
        let src = tempdir()?;
        let info = make_media_info(src.path(), "photo.png")?;

        let out = tempdir()?;
        let out_utf8 = Utf8PathBuf::from(out.path().to_str().unwrap());
        let sub = out.path().join("2024").join("01");
        fs::create_dir_all(&sub)?;
        fs::write(sub.join("photo.png"), b"placeholder")?;

        let (_, target) = generate_unique_name(&info, &out_utf8)?
            .expect("unique name should be generated");
        assert!(target.ends_with("photo_1.png"), "got {target}");
        Ok(())
    }

    #[test]
    fn generate_unique_name_none_after_10_collisions() -> tc::Result {
        let src = tempdir()?;
        let info = make_media_info(src.path(), "photo.png")?;

        let out = tempdir()?;
        let out_utf8 = Utf8PathBuf::from(out.path().to_str().unwrap());
        let sub = out.path().join("2024").join("01");
        fs::create_dir_all(&sub)?;
        fs::write(sub.join("photo.png"), b"")?;
        for i in 1..10 {
            fs::write(sub.join(format!("photo_{i}.png")), b"")?;
        }

        let res = generate_unique_name(&info, &out_utf8)?;
        assert!(res.is_none(), "should exhaust after 10 collisions: got {res:?}");
        Ok(())
    }

    #[test]
    fn do_copy_errors_when_unique_name_exhausted() -> tc::Result {
        let src = tempdir()?;
        let info = make_media_info(src.path(), "photo.png")?;

        let out = tempdir()?;
        let out_utf8 = Utf8PathBuf::from(out.path().to_str().unwrap());
        let sub = out.path().join("2024").join("01");
        fs::create_dir_all(&sub)?;
        fs::write(sub.join("photo.png"), b"")?;
        for i in 1..10 {
            fs::write(sub.join(format!("photo_{i}.png")), b"")?;
        }

        let mut idx = super::super::entities::file_index::Index::new();
        let err = do_copy(&info, &out_utf8, &mut idx, false, false).unwrap_err();
        assert!(err.to_string().contains("无法为"), "got: {err}");
        Ok(())
    }

    #[test]
    fn do_copy_dry_run_reports_target_but_writes_nothing() -> tc::Result {
        let src = tempdir()?;
        let info = make_media_info(src.path(), "photo.png")?;

        let out = tempdir()?;
        let out_utf8 = Utf8PathBuf::from(out.path().to_str().unwrap());
        let mut idx = super::super::entities::file_index::Index::new();

        let did_copy = do_copy(&info, &out_utf8, &mut idx, true, false)?;
        assert!(did_copy, "dry run should report a planned copy");
        let entries: Vec<_> = fs::read_dir(out.path())?.collect();
        assert!(entries.is_empty(), "dry run must not write");
        Ok(())
    }
}
