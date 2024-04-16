use std::{fs, io};

use tracing::info;

use super::entities::file_checksum;
use super::entities::file_index;

pub fn copy(sources: Vec<String>, output: String) -> io::Result<()> {
    fs::create_dir_all(output.as_str())?;
    let output = fs::canonicalize(output.as_str())?;

    let mut out = file_index::FileIndex::new();
    out.visit_dir(output.to_str().unwrap());

    let mut source = file_index::FileIndex::new();
    for path in sources {
        source.visit_dir(path.as_str());
    }

    info!(
        "Files: {}, FastChecksums: {}, BytesRead: {}",
        source.files.len(),
        source.fast_checksums.len(),
        source.bytes_read(),
    );

    for (name, checksum) in source.files {
        let exists = match out.get(checksum.short) {
            Some(paths) => {
                let mut exists = false;
                for path in paths {
                    let cs = out.files.get(path).unwrap();
                    if cs == &checksum {
                        exists = true;
                        break;
                    }
                }
                exists
            }
            None => false,
        };

        if !exists {
            let name = std::path::PathBuf::from(name);
            // let modified_time = fs::metadata(name.as_path())?.modified()?;
            // TODO: get modified time as path
            let to = output.join(name.file_name().unwrap());
            fs::copy(name.to_str().unwrap(), to.as_path())?;
            _ = out.add(file_checksum::FileChecksum::new_path(to.as_path())?);
        }
    }

    // info!("BytesRead: {}", source.bytes_read());
    Ok(())
}
