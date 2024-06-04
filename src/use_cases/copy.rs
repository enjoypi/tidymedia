use std::fs;
use std::io;
use std::io::ErrorKind;

use camino::Utf8Component;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use time::error;
use time::OffsetDateTime;
use time::UtcOffset;
use tracing::{error, warn};
use tracing::info;
use tracing::trace;

use super::entities::file_index::Index;
use super::entities::file_info::{full_path, Info};

const CST: Result<UtcOffset, error::ComponentRange> = UtcOffset::from_hms(8, 0, 0);
const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

pub fn copy(sources: Vec<Utf8PathBuf>, output: Utf8PathBuf, dry_run: bool, remove: bool) -> io::Result<()> {
    if !dry_run {
        fs::create_dir_all(output.as_str())?;
    }
    let output_path = full_path(output.as_str())?;

    let mut source = Index::new();
    sources.iter().for_each(|s| {
        source.visit_dir(s.as_str());
        if let Err(e) = source.parse_exif() {
            error!("{}", e);
        }
    });
    trace!("Files: {:#?}", source);

    info!(
        "Files: {}, UniqueFiles: {}, BytesRead: {}",
        source.files().len(),
        source.similar_files().len(),
        source.bytes_read(),
    );

    let mut output_index = Index::new();
    output_index.visit_dir(output_path.as_str());

    let mut copied = 0;
    source.files().iter().for_each(|(_, src)| {
        if let Err(e) = do_copy(src, &output_path, &mut output_index, dry_run, remove) {
            error!("{}", e)
        } else {
            copied += 1;
        }
    });

    info!("Copied files: {}", copied);
    Ok(())
}

fn do_copy(
    src: &Info,
    output_dir: &Utf8PathBuf,
    output_index: &mut Index,
    dry_run: bool,
    remove: bool,
) -> io::Result<()> {
    let full_path = src.full_path.as_str();

    if let Some(dup) = output_index.exists(src)? {
        trace!("SAME_FILE\t[{}]\t[{}]", full_path, dup);
        if remove && !dry_run {
            return fs::remove_file(full_path);
        }
        return Ok(());
    }

    if !src.is_media() {
        warn!("IGNORED\t[{}]", full_path);
        return Ok(());
    }

    if let Some((target_dir, target)) = generate_unique_name(src, output_dir)? {
        if dry_run {
            trace!("COPIED\t[{}]\t[{}]", full_path, target);
            return Ok(());
        }

        fs::create_dir_all(target_dir.as_str())?;
        let target = target.as_str();

        if remove {
            fs::rename(full_path, target)?;
            trace!("MOVED\t[{}]\t[{}]", full_path, target);
            // TODO different disk can't use rename
        } else {
            if fs::copy(full_path, target)? != src.size {
                error!("COPY_FAILED\t[{}]\t[{}]", full_path, target);
                return Ok(());
            }
            trace!("COPIED\t[{}]\t[{}]", full_path, target);
        }

        _ = output_index.add(Info::from(target)?);

        Ok(())
    } else {
        Err(io::Error::new(
            ErrorKind::Other,
            format!(
                "Failed to generate unique name for {}",
                src.full_path.as_str()
            ),
        ))
    }
}

fn generate_unique_name(
    src_file: &Info,
    output_dir: &Utf8PathBuf,
) -> io::Result<Option<(String, String)>> {
    let full_path = Utf8Path::new(src_file.full_path.as_str());
    let file_name = full_path
        .file_name()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
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

            #[cfg(target_os = "windows")]
                let sub_dir = sub_dir.replace('\\', "/");
            #[cfg(target_os = "windows")]
                let target = target.replace('\\', "/");

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
