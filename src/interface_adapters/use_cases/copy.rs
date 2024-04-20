use std::fs;
use std::io;
use std::io::Error;
use std::path::Path;

use time::error;
use time::OffsetDateTime;
use time::UtcOffset;
use tracing::error;
use tracing::info;

use super::entities::file_index::Index;
use super::entities::file_info::Info;

pub fn copy(sources: Vec<String>, output: String) -> io::Result<()> {
    fs::create_dir_all(output.as_str())?;
    let output_dir = fs::canonicalize(output.as_str())?;

    let mut output_index = Index::new();
    output_index.visit_dir(output_dir.to_str().unwrap());

    let mut source = Index::new();
    for path in sources {
        source.visit_dir(path.as_str());
    }

    info!(
        "Files: {}, FastHashs: {}, BytesRead: {}",
        source.files().len(),
        source.similar_files().len(),
        source.bytes_read(),
    );

    for (path, src) in source.files() {
        if output_index.exists(src)? {
            continue;
        }

        if let Some((target_dir, target)) = generate_unique_name(src, &output_dir)? {
            fs::create_dir_all(target_dir.as_str())?;
            let target = target.as_str();

            if fs::copy(path, target)? != src.size {
                error!("Copy failed: {} to {}", path, target);
                continue;
            }

            _ = output_index.add(Info::from(target)?);
        } else {
            error!(
                "Failed to generate unique name for {}",
                src.full_path.as_str()
            );
        }
    }

    // info!("BytesRead: {}", source.bytes_read());
    Ok(())
}

fn generate_unique_name(
    src_file: &Info,
    output_dir: &Path,
) -> io::Result<Option<(String, String)>> {
    let full_path = Path::new(src_file.full_path.as_str());
    let file_name = full_path
        .file_name()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid file name",
        ))?
        .to_str()
        .ok_or(Error::new(io::ErrorKind::InvalidInput, "Invalid file name"))?;

    let file_stem = full_path.file_stem().unwrap().to_string_lossy().to_string();
    let ext = full_path.extension().unwrap().to_string_lossy().to_string();

    let modified_time = src_file.modified_time()?;

    let dt = OffsetDateTime::from(modified_time).to_offset(CST.expect("CST"));
    let month = (dt.month() as u8).to_string();
    let year = dt.year().to_string();

    let sub_dir = output_dir.join(year).join(month);

    // generate unique name by adding a number suffix
    for i in 0..10 {
        let target = if i <= 0 {
            sub_dir.join(file_name)
        } else {
            let mut file_name = file_stem.clone();

            file_name.push('_');
            file_name.push_str(i.to_string().as_str());
            file_name.push('.');
            file_name.push_str(ext.as_str());
            sub_dir.join(file_name)
        };

        if !target.exists() {
            return Ok(Some((
                sub_dir.to_str().unwrap().to_string(),
                target.to_str().unwrap().to_string(),
            )));
        }
    }
    Ok(None)
}

const CST: Result<UtcOffset, error::ComponentRange> = UtcOffset::from_hms(8, 0, 0);
