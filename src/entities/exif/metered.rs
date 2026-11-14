use std::io::{self, Read, Seek, SeekFrom};
use std::sync::OnceLock;

use tracing::warn;

use crate::entities::backend::MediaReader;
use crate::entities::memory::{Budget, Level, Reservation};

const BUFFER_GROWTH_FACTOR: u64 = 3;
const DEFAULT_FREE_BYTES: u64 = 1 << 20;
const DEFAULT_MAX_BYTES: u64 = 64 << 20;

static UNLIMITED_WORK: Budget = Budget::new(Level::Work, u64::MAX);
static PARSE_LIMITS: OnceLock<ParseLimits> = OnceLock::new();

#[derive(Clone, Copy)]
pub struct ParseLimits {
    pub work: &'static Budget,
    pub free_bytes: u64,
    pub max_bytes: u64,
}

pub fn set_parse_limits(limits: ParseLimits) {
    let _ = PARSE_LIMITS.set(limits);
}

pub(super) fn parse_limits() -> ParseLimits {
    PARSE_LIMITS.get().copied().unwrap_or(ParseLimits {
        work: &UNLIMITED_WORK,
        free_bytes: DEFAULT_FREE_BYTES,
        max_bytes: DEFAULT_MAX_BYTES,
    })
}

pub(super) struct MeteredReader {
    inner: Box<dyn MediaReader>,
    limits: ParseLimits,
    consumed: u64,
    held: Option<Reservation<'static>>,
}

impl MeteredReader {
    pub(super) fn new(inner: Box<dyn MediaReader>, limits: ParseLimits) -> Self {
        Self {
            inner,
            limits,
            consumed: 0,
            held: None,
        }
    }

    fn too_large(&self) -> io::Error {
        warn!(
            feature = "exif",
            operation = "parse_metadata",
            result = "metadata_too_large",
            consumed = self.consumed,
            max_bytes = self.limits.max_bytes,
            "metadata exceeds the parse budget; falling back to other time sources"
        );
        io::Error::new(
            io::ErrorKind::FileTooLarge,
            "metadata exceeds the parse budget",
        )
    }
}

impl Read for MeteredReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.consumed = self.consumed.saturating_add(n as u64);
        if self.consumed > self.limits.max_bytes {
            return Err(self.too_large());
        }
        if self.consumed > self.limits.free_bytes && self.held.is_none() {
            let need = self.limits.max_bytes.saturating_mul(BUFFER_GROWTH_FACTOR);
            let held = self
                .limits
                .work
                .reserve(need)
                .map_err(|_| self.too_large())?;
            self.held = Some(held);
        }
        Ok(n)
    }
}

impl Seek for MeteredReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[cfg(test)]
#[path = "metered_tests.rs"]
mod tests;
