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
use super::entities::file_info::{full_path, Info};

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
        if let Err(e) = source.parse_exif() {
            error!("解析 Exif 信息失败：{}", e);
        }
    });

    info!("源目录中有不重复文件 {} 个", source.similar_files().len());

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
    source.similar_files().iter().for_each(|(_, src)| {
        let src = src.iter().next();
        if src.is_none() {
            return;
        }
        let src = source.files().get(src.unwrap());
        if src.is_none() {
            return;
        }

        let src = src.unwrap();
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
        source.similar_files().len(),
        copied,
        ignored,
        failed
    );
    Ok(())
}

fn do_copy(
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

fn generate_unique_name(
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
    fn test_extract_valuable_name() {
        let path = Utf8Path::new("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z");
        assert_eq!(extract_valuable_name(path), "");

        let path = Utf8Path::new("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/中文/abc");
        assert_eq!(extract_valuable_name(path), "中文");

        let path = Utf8Path::new("D:\\todo\\Pictures\\ 高一元 旦晚会 \\102_PANA\\P1020486.MP4");
        assert_eq!(extract_valuable_name(path), " 高一元 旦晚会 ");

        let path = Utf8Path::new("D:\\todo\\Pictures\\a高一 元旦晚会\\102_PANA\\P1020486.MP4");
        assert_eq!(extract_valuable_name(path), "a高一 元旦晚会");

        let path = Utf8Path::new("D:\\todo\\Pictures\\高一 元旦晚会 z\\102_PANA\\P1020486.MP4");
        assert_eq!(extract_valuable_name(path), "高一 元旦晚会 z");

        let path = Utf8Path::new("D:\\todo\\Pictures\\_高一 元旦晚会\\102_PANA\\P1020486.MP4");
        assert_eq!(extract_valuable_name(path), "_高一 元旦晚会");

        let path = Utf8Path::new("D:\\todo\\Pictures\\高一 元旦晚会_\\102_PANA\\P1020486.MP4");
        assert_eq!(extract_valuable_name(path), "高一 元旦晚会_");
    }
}
