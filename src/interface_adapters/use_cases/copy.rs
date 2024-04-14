use std::{fs, io};

use tracing::info;

use super::entities::file_index;

pub fn copy(sources: Vec<String>, output: String) -> io::Result<()> {
    fs::create_dir_all(output.as_str())?;

    let mut out = file_index::FileIndex::new();
    out.visit_dir(output.as_str());

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

    info!("BytesRead: {}", source.bytes_read());
    Ok(())
}
