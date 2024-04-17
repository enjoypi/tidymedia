use std::{fs, io};

use tracing::info;

use super::entities::file_index;
use super::entities::file_meta;

pub fn copy(sources: Vec<String>, output: String) -> io::Result<()> {
    fs::create_dir_all(output.as_str())?;
    let output = fs::canonicalize(output.as_str())?;

    let mut out = file_index::Index::new();
    out.visit_dir(output.to_str().unwrap());

    let mut source = file_index::Index::new();
    for path in sources {
        source.visit_dir(path.as_str());
    }

    info!(
        "Files: {}, FastChecksums: {}, BytesRead: {}",
        source.files().len(),
        source.unique_files().len(),
        source.bytes_read(),
    );

    for (name, meta) in source.files() {
        let exists = match out.unique_files().get(&meta.short) {
            Some(paths) => {
                let mut exists = false;
                for path in paths {
                    let cs = out.files().get(path).unwrap();
                    if cs == meta {
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
            let _modified_time = meta.modified_time()?;
            // TODO: get modified time as path
            let to = output.join(name.file_name().unwrap());

            // 如果目标文件已经存在，再次判断是否相同，不相同则改名
            if to.exists() {
                let cs = out.files().get(to.to_str().unwrap()).unwrap();
                if cs != meta {
                    let mut i = 1;
                    let mut to = to.clone();
                    while to.exists() {
                        to = output.join(format!(
                            "{}-{}",
                            name.file_name().unwrap().to_str().unwrap(),
                            i
                        ));
                        i += 1;
                    }
                    fs::copy(name.to_str().unwrap(), to.as_path())?;
                    _ = out.add(file_meta::Meta::new_path(to.as_path())?);
                }
            } else {
                fs::copy(name.to_str().unwrap(), to.as_path())?;
                _ = out.add(file_meta::Meta::new_path(to.as_path())?);
            }
            fs::copy(name.to_str().unwrap(), to.as_path())?;
            _ = out.add(file_meta::Meta::new_path(to.as_path())?);
        }
    }

    // info!("BytesRead: {}", source.bytes_read());
    Ok(())
}
