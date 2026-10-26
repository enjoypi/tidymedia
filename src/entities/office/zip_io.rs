use std::io::{Read, Seek};

use crate::entities::backend::MediaReader;

pub(super) fn open_zip(
    reader: &mut dyn MediaReader,
) -> Option<zip::ZipArchive<&mut dyn MediaReader>> {
    zip::ZipArchive::new(reader).ok()
}

pub(super) fn read_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    cap: u64,
) -> Option<Vec<u8>> {
    let entry = archive.by_name(name).ok()?;
    let mut content = Vec::new();
    entry.take(cap).read_to_end(&mut content).ok()?;
    Some(content)
}

pub(super) fn entry_names<R: Read + Seek>(archive: &zip::ZipArchive<R>) -> Vec<String> {
    archive.file_names().map(str::to_string).collect()
}
