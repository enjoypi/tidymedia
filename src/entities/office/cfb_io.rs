use std::io::Read;
use std::path::{Path, PathBuf};

use crate::entities::backend::MediaReader;

pub(super) fn open_cfb(
    reader: &mut dyn MediaReader,
) -> Option<cfb::CompoundFile<&mut dyn MediaReader>> {
    cfb::CompoundFile::open(reader).ok()
}

pub(super) fn stream_paths<F, P>(comp: &cfb::CompoundFile<F>, mut filter: P) -> Vec<PathBuf>
where
    P: FnMut(&str) -> bool,
{
    comp.walk()
        .filter(|e| e.is_stream() && filter(e.name()))
        .map(|e| e.path().to_path_buf())
        .collect()
}

pub(super) fn open_stream<F: std::io::Seek, P: AsRef<Path>>(
    comp: &mut cfb::CompoundFile<F>,
    path: P,
) -> Option<cfb::Stream<'_, F>> {
    comp.open_stream(path).ok()
}

pub(super) fn read_stream<R: Read>(mut stream: R) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}
