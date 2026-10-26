use std::io;
use std::io::Read;

use sha2::Digest;
use sha2::Sha512;

use super::info::Info;
use super::paths::full_path;
use super::streams::FAST_READ_SIZE;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::SecureHash;
use crate::entities::uri::Location;

impl Info {
    pub fn from(path: &str) -> io::Result<Self> {
        let fp = full_path(path)?;
        Self::open(&Location::Local(fp), LocalBackend::arc())
    }

    pub fn exif(&self) -> Option<&crate::entities::exif::Exif> {
        self.exif.as_ref()
    }
}

pub(super) fn fast_hash(path: &str) -> (usize, u64, u64) {
    let mut file =
        std::fs::File::open(path).expect("test helper: fixture file must open for fast_hash");
    let mut buffer = [0; FAST_READ_SIZE];
    let bytes_read = file
        .read(&mut buffer)
        .expect("test helper: fixture file must read for fast_hash");
    let short = wyhash::wyhash(&(buffer[..bytes_read]), 0);
    let full = xxhash_rust::xxh3::xxh3_64(&(buffer[..bytes_read]));
    (bytes_read, short, full)
}

pub(super) fn full_hash(path: &str) -> (usize, u64) {
    let file =
        std::fs::File::open(path).expect("test helper: fixture file must open for full_hash");
    let mmap = unsafe {
        memmap2::Mmap::map(&file).expect("test helper: fixture file must mmap for full_hash")
    };
    (mmap.len(), xxhash_rust::xxh3::xxh3_64(&mmap))
}

pub(super) fn secure_hash(path: &str) -> (usize, SecureHash) {
    let file =
        std::fs::File::open(path).expect("test helper: fixture file must open for secure_hash");
    let mmap = unsafe {
        memmap2::Mmap::map(&file).expect("test helper: fixture file must mmap for secure_hash")
    };
    (mmap.len(), Sha512::digest(&mmap))
}
