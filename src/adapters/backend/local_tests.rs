use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

use camino::Utf8PathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::LocalBackend;
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::uri::Location;

fn local(p: impl AsRef<std::path::Path>) -> Location {
    Location::Local(Utf8PathBuf::from_path_buf(p.as_ref().to_path_buf()).unwrap())
}

fn smb_uri() -> Location {
    Location::parse("smb://nas/share/x").unwrap()
}

include!("local_part1_tests.rs");
include!("local_part2_tests.rs");
