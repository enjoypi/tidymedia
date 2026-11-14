use std::sync::Arc;

use camino::Utf8PathBuf;

use super::*;
use crate::entities::backend::EntryKind;
use crate::entities::uri::Location;

const COST: EntryCost = EntryCost {
    base: 100,
    path_copies: 2,
};

fn entry(path: &str, size: u64) -> Entry {
    Entry {
        location: Location::Local(Utf8PathBuf::from(path)),
        size,
        kind: EntryKind::File,
    }
}

#[test]
fn entry_cost_scales_with_path_length() {
    assert_eq!(COST.of(&entry("/abc", 1)), 108);
}

#[test]
fn buckets_stay_in_range_and_are_stable() {
    for size in [0, 1, 2, 4096, u64::MAX] {
        assert!(bucket_of(size) < BUCKETS);
        assert_eq!(bucket_of(size), bucket_of(size));
    }
}

#[test]
fn charging_takes_until_ledger_is_full() {
    let ledger: &'static Ledger = Box::leak(Box::new(Ledger::new(250)));
    let charging = Charging { ledger, cost: COST };
    assert!(charging.counts_shared_stats());
    assert_eq!(charging.admit(&entry("/abc", 1)), Admit::Take);
    assert_eq!(charging.admit(&entry("/abc", 2)), Admit::Take);
    assert_eq!(charging.admit(&entry("/abc", 3)), Admit::Overflow);
}

#[test]
fn tally_accumulates_by_bucket_and_names_without_taking() {
    let hist = Histogram::new();
    let tally = Tally {
        hist: Arc::clone(&hist),
        cost: COST,
        name_cost: EntryCost {
            base: 10,
            path_copies: 1,
        },
    };
    assert!(tally.counts_shared_stats());
    assert_eq!(tally.admit(&entry("/abc", 7)), Admit::Skip);
    assert_eq!(tally.admit(&entry("/abc", 7)), Admit::Skip);
    let snapshot = hist.snapshot();
    assert_eq!(snapshot[bucket_of(7)], 216);
    assert_eq!(snapshot.iter().sum::<u64>(), 216);
    assert_eq!(hist.names(), 28);
}
