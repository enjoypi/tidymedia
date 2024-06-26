use camino::Utf8PathBuf;
use tracing::error;
use tracing::info;

use super::entities::common;
use super::entities::file_index;
use super::entities::file_info;

pub fn find_duplicates(
    fast: bool,
    sources: Vec<Utf8PathBuf>,
    output: Option<Utf8PathBuf>,
) -> common::Result<()> {
    let mut index = file_index::Index::new();

    if let Some(output) = output.clone() {
        // check if output is directory
        // if not, create directory
        // the code is
        let output = std::path::Path::new(&output);
        if !output.is_dir() {
            error!("output is not a directory");
            return Ok(());
        }
    }

    for path in sources {
        index.visit_dir(path.as_str());
    }

    info!(
        "Files: {}, Similar Files: {}, Bytes Read: {}",
        index.files().len(),
        index.similar_files().len(),
        index.bytes_read(),
    );

    let same = if fast {
        index.fast_search_same()
    } else {
        index.search_same()
    };
    info!("Same: {}", same.len());

    match output {
        Some(output) => {
            let output = file_info::full_path(output.as_str()).unwrap();
            let output = output.as_str();
            for (size, paths) in same.iter().rev() {
                println!("{}SIZE {}\r", comment(), size);
                for path in paths.iter() {
                    if path.starts_with(output) {
                        println!("{}{} \"{}\"\r", comment(), rm(), path);
                    } else {
                        println!("{} \"{}\"\r", rm(), path);
                    }
                }
                println!()
            }
        }
        _ => {
            for (size, paths) in same.iter().rev() {
                println!("{}SIZE {}\r", comment(), size);
                for path in paths.iter() {
                    println!("{}{} \"{}\"\r", comment(), rm(), path);
                }
                println!()
            }
        }
    }

    info!("Bytes Read: {}", index.bytes_read());
    Ok(())
}

#[cfg(target_os = "windows")]
fn comment() -> &'static str {
    ":"
}

#[cfg(not(target_os = "windows"))]
fn comment() -> &'static str {
    "#"
}

#[cfg(target_os = "windows")]
fn rm() -> &'static str {
    "DEL"
}

#[cfg(not(target_os = "windows"))]
fn rm() -> &'static str {
    "rm"
}
