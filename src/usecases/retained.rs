use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::entities::backend::Entry;
use crate::entities::common;
use crate::entities::file_index::{Admission, Admit};
use crate::entities::memory::{CapacityExceeded, Ledger};

const BUCKET_BITS: u32 = 16;
const BUCKETS: usize = 1 << BUCKET_BITS;
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntryCost {
    pub base: u64,
    pub path_copies: u64,
}

impl EntryCost {
    #[must_use]
    pub(crate) fn of(&self, entry: &Entry) -> u64 {
        self.of_path(&entry.location.display())
    }

    #[must_use]
    pub(crate) fn of_path(&self, path: &str) -> u64 {
        self.base
            .saturating_add((path.len() as u64).saturating_mul(self.path_copies))
    }
}

#[must_use]
pub(crate) fn bucket_of(size: u64) -> usize {
    usize::try_from(size.wrapping_mul(GOLDEN) >> (64 - BUCKET_BITS)).unwrap_or(0)
}

pub(crate) struct Charging {
    pub ledger: &'static Ledger,
    pub cost: EntryCost,
}

impl Admission for Charging {
    fn admit(&self, entry: &Entry) -> Admit {
        match self.ledger.try_charge(self.cost.of(entry)) {
            Ok(()) => Admit::Take,
            Err(_) => Admit::Overflow,
        }
    }

    fn counts_shared_stats(&self) -> bool {
        true
    }
}

pub(crate) struct Histogram {
    charge: Vec<AtomicU64>,
    names: AtomicU64,
}

impl Histogram {
    #[must_use]
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            charge: (0..BUCKETS).map(|_| AtomicU64::new(0)).collect(),
            names: AtomicU64::new(0),
        })
    }

    #[must_use]
    pub(crate) fn snapshot(&self) -> Vec<u64> {
        self.charge
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect()
    }

    #[must_use]
    pub(crate) fn names(&self) -> u64 {
        self.names.load(Ordering::Relaxed)
    }
}

pub(crate) struct Tally {
    pub hist: Arc<Histogram>,
    pub cost: EntryCost,
    pub name_cost: EntryCost,
}

impl Admission for Tally {
    fn admit(&self, entry: &Entry) -> Admit {
        self.hist.charge[bucket_of(entry.size)].fetch_add(self.cost.of(entry), Ordering::Relaxed);
        self.hist
            .names
            .fetch_add(self.name_cost.of(entry), Ordering::Relaxed);
        Admit::Skip
    }

    fn counts_shared_stats(&self) -> bool {
        true
    }
}

pub(crate) fn memory_error(e: CapacityExceeded) -> common::Error {
    common::Error::Io(io::Error::new(
        io::ErrorKind::OutOfMemory,
        format!(
            "memory budget cannot hold {} bytes of index entries (capacity {} bytes); split the input into smaller batches",
            e.need, e.capacity
        ),
    ))
}

#[cfg(test)]
#[path = "retained_tests.rs"]
mod tests;
