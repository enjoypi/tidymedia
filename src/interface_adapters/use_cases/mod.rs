pub mod entities;
use entities::{file_checksum, file_index};
use tracing::{error, info};

pub fn find_duplicates(fast: bool, dirs: Vec<String>, output: Option<String>) {
    let mut index = file_index::FileIndex::new();

    if let Some(output) = output.clone() {
        // check if output is directory
        // if not, create directory
        // the code is
        let output = std::path::Path::new(&output);
        if !output.is_dir() {
            error!("output is not a directory");
            return;
        }
    }

    for path in dirs {
        index.visit_dir(path.as_str());
    }

    info!(
        "Files: {}, FastChecksums: {}, BytesRead: {}",
        index.files.len(),
        index.fast_checksums.len(),
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
            let output =
                file_checksum::FileChecksum::get_full_path(std::path::Path::new(&output)).unwrap();
            let output = output.as_str();
            for (size, paths) in same.iter().rev() {
                println!(":SIZE {}\r", size);
                for path in paths.iter() {
                    if path.starts_with(output) {
                        println!(":DEL \"{}\"\r", path);
                    } else {
                        println!("DEL \"{}\"\r", path);
                    }
                }
                println!()
            }
        }
        _ => {
            for (size, paths) in same.iter().rev() {
                println!(":SIZE {}\r", size);
                for path in paths.iter() {
                    println!(":DEL \"{}\"\r", path);
                }
                println!()
            }
        }
    }

    info!("BytesRead: {}", index.bytes_read());
}
