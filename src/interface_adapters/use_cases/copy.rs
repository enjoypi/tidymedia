use std::path::PathBuf;
use std::string::ToString;
use std::{fs, io};

use time::{error, OffsetDateTime, UtcOffset};
use tracing::{error, info};

use super::entities::file_index::Index;
use super::entities::file_info::Info;

pub fn copy(sources: Vec<String>, output: String) -> io::Result<()> {
    fs::create_dir_all(output.as_str())?;
    let output = fs::canonicalize(output.as_str())?;

    let mut out = Index::new();
    out.visit_dir(output.to_str().unwrap());

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

    for (path, meta) in source.files() {
        if out.exists(meta)? {
            continue;
        }

        let target = generate_unique_name(meta, &output, &out)?;
        let target = target.as_str();

        if fs::copy(path, target)? != meta.size {
            error!("Copy failed: {} to {}", path, target);
            continue;
        }

        _ = out.add(Info::from(target)?);
    }

    // info!("BytesRead: {}", source.bytes_read());
    Ok(())
}

fn generate_unique_name(
    src_file: &Info,
    output_path: &PathBuf,
    output: &Index,
) -> io::Result<String> {
    let name = std::path::PathBuf::from(src_file.path.as_str());
    let modified_time = src_file.modified_time()?;

    let dt = OffsetDateTime::from(modified_time).to_offset(CST.expect("CST"));
    let month = dt.month().to_string();
    let year = dt.year().to_string();

    let target = output_path
        .join(year)
        .join(month)
        .join(name.file_name().unwrap());
    let target_str = target.to_str().unwrap();

    if output.files().contains_key(target_str) {
        return Ok(EMPTY_STRING);
    }
    // if !output.exists(target) {
    //     return Ok(target.to_str().unwrap().to_string());
    // }

    Ok(target.to_str().unwrap().to_string())
}

const CST: Result<UtcOffset, error::ComponentRange> = UtcOffset::from_hms(8, 0, 0);
const EMPTY_STRING: String = String::new();
