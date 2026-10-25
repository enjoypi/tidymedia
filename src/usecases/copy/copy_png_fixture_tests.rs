use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn write_unique_png(dir: &Path, name: &str, unique: u64) -> PathBuf {
    let mut bytes = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];
    bytes.extend_from_slice(&unique.to_le_bytes());
    bytes.resize(256, 0);
    let p = dir.join(name);
    fs::write(&p, &bytes).unwrap();
    let ts = filetime::FileTime::from_unix_time(crate::entities::test_common::FIXED_MEDIA_MTIME, 0);
    filetime::set_file_mtime(&p, ts).unwrap();
    p
}
