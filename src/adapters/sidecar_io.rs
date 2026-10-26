use std::io;

use camino::Utf8Path;

use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::media_time::Candidate;
use crate::entities::uri::Location;

#[must_use]
pub fn discover(media_path: &Utf8Path) -> Vec<Candidate> {
    let backend = LocalBackend::arc();
    super::discover_with_backend(&Location::Local(media_path.to_path_buf()), &backend)
}

pub(super) fn read(loc: &Location, backend: &dyn Backend) -> io::Result<String> {
    backend.read_to_string(loc)
}
