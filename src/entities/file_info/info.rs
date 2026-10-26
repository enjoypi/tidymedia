#[path = "time.rs"]
pub(super) mod time;

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use chrono::FixedOffset;
use parking_lot::Mutex;

use super::streams::{fast_hash_stream, full_hash_stream, secure_hash_stream};
use crate::entities::backend::{Backend, EntryKind, Metadata as BackendMetadata};
use crate::entities::uri::Location;
use crate::entities::{SecureHash, exif, media_time};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lazy {
    bytes_read: u64,
    full: bool,
    hash: u64,
    secure_hash: SecureHash,
}

impl Lazy {
    fn new(bytes_read: u64, hash: u64) -> Self {
        Self {
            bytes_read,
            hash,
            full: false,
            secure_hash: SecureHash::default(),
        }
    }
}

pub struct Info {
    pub fast_hash: u64,
    pub full_path: Utf8PathBuf,
    pub size: u64,

    location: Location,
    backend: Arc<dyn Backend>,

    pub(super) exif: Option<exif::Exif>,
    extra_candidates: Vec<media_time::Candidate>,
    category: Option<String>,
    lazy: Mutex<Lazy>,
    meta: BackendMetadata,
}

impl std::fmt::Debug for Info {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fast_hash: {}, size: {}\n{:#?}",
            self.fast_hash, self.size, self.exif
        )
    }
}

impl Info {
    pub fn open(loc: &Location, backend: Arc<dyn Backend>) -> io::Result<Self> {
        let meta = backend.metadata(loc)?;
        ensure_hashable(&meta, loc)?;
        let mut reader = backend.open_read(loc)?;
        let (bytes_read, first_hash, second_hash) = fast_hash_stream(reader.as_mut())?;
        let full_path: Utf8PathBuf = match loc {
            Location::Local(p) => p.clone(),
            other @ Location::Smb { .. } => other.display().into(),
        };
        Ok(Self {
            fast_hash: first_hash,
            full_path,
            size: meta.size,
            location: loc.clone(),
            backend,
            exif: None,
            extra_candidates: Vec::new(),
            category: None,
            lazy: Mutex::new(Lazy::new(bytes_read as u64, second_hash)),
            meta,
        })
    }

    pub fn bytes_read(&self) -> u64 {
        self.lazy.lock().bytes_read
    }

    pub fn location(&self) -> &Location {
        &self.location
    }

    pub fn backend(&self) -> Arc<dyn Backend> {
        Arc::clone(&self.backend)
    }

    pub fn calc_full_hash(&self) -> io::Result<u64> {
        let mut l = self.lazy.lock();
        if l.full {
            return Ok(l.hash);
        }
        let mut reader = self.backend.open_read(&self.location)?;
        let (bytes_read, full) = full_hash_stream(reader.as_mut())?;
        l.hash = full;
        l.bytes_read += bytes_read;
        l.full = true;
        Ok(full)
    }

    pub(super) fn full_hash(&self) -> u64 {
        self.lazy.lock().hash
    }

    pub fn secure_hash(&self) -> io::Result<SecureHash> {
        let mut l = self.lazy.lock();
        if l.secure_hash != SecureHash::default() {
            return Ok(l.secure_hash);
        }
        let mut reader = self.backend.open_read(&self.location)?;
        let (bytes_read, secure) = secure_hash_stream(reader.as_mut())?;
        l.bytes_read += bytes_read;
        l.secure_hash = secure;
        Ok(secure)
    }

    pub fn exif_ref(&self) -> Option<&exif::Exif> {
        self.exif.as_ref()
    }

    pub fn set_exif(&mut self, exif: exif::Exif) {
        self.exif = Some(exif);
    }

    pub fn add_candidates(&mut self, candidates: Vec<media_time::Candidate>) {
        self.extra_candidates.extend(candidates);
    }

    pub fn cloned_at(&self, new_loc: Location, new_backend: Arc<dyn Backend>) -> Self {
        let new_full_path: Utf8PathBuf = match &new_loc {
            Location::Local(p) => p.clone(),
            other @ Location::Smb { .. } => other.display().into(),
        };
        let lazy_snapshot = *self.lazy.lock();
        Self {
            fast_hash: self.fast_hash,
            full_path: new_full_path,
            size: self.size,
            location: new_loc,
            backend: new_backend,
            exif: self.exif.clone(),
            extra_candidates: self.extra_candidates.clone(),
            category: self.category.clone(),
            lazy: Mutex::new(lazy_snapshot),
            meta: self.meta.clone(),
        }
    }

    pub fn create_time(
        &self,
        valid_threshold_secs: u64,
        default_offset: FixedOffset,
    ) -> std::time::SystemTime {
        time::create_time(self, valid_threshold_secs, default_offset)
    }

    pub fn media_time_decision(
        &self,
        default_offset: FixedOffset,
    ) -> Option<media_time::MediaTimeDecision> {
        time::media_time_decision(self, default_offset)
    }

    pub fn is_media(&self) -> bool {
        self.exif.as_ref().is_some_and(exif::Exif::is_media)
    }

    pub fn is_office(&self) -> bool {
        self.exif.as_ref().is_some_and(exif::Exif::is_office)
    }

    pub fn set_category(&mut self, category: String) {
        self.category = Some(category);
    }

    pub fn category_ref(&self) -> Option<&str> {
        self.category.as_deref()
    }
}

fn ensure_hashable(meta: &crate::entities::backend::Metadata, loc: &Location) -> io::Result<()> {
    if meta.kind != EntryKind::File {
        return Err(io::Error::other(format!(
            "{} is a directory",
            loc.display()
        )));
    }
    if meta.size == 0 {
        return Err(io::Error::other(format!("{} is empty", loc.display())));
    }
    Ok(())
}

impl PartialEq for Info {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.fast_hash == other.fast_hash
            && self.full_hash() == other.full_hash()
    }
}
