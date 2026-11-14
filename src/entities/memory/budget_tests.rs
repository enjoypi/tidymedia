use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use super::*;

fn wait_for(budget: &Budget, waiting: u64) {
    while budget.waiting() != waiting {
        thread::yield_now();
    }
}

#[test]
fn reserve_rejects_request_larger_than_capacity() {
    let budget = Budget::new(Level::Work, 10);
    let err = budget.reserve(11).err().unwrap();
    assert_eq!(
        err,
        TooLarge {
            need: 11,
            capacity: 10
        }
    );
    assert_eq!(err.to_string(), "needs 11 bytes, memory budget is 10 bytes");
    assert_eq!(budget.available(), 10);
}

#[test]
fn reservation_returns_bytes_on_drop() {
    let budget = Budget::new(Level::Work, 10);
    let held = budget.reserve(10).unwrap();
    assert_eq!(budget.available(), 0);
    drop(held);
    assert_eq!(budget.available(), 10);
    assert!(budget.reserve(10).is_ok());
}

#[test]
fn waiters_are_served_first_in_first_out() {
    let budget = Budget::new(Level::Work, 10);
    let order = Mutex::new(Vec::new());
    let head = budget.reserve(8).unwrap();
    thread::scope(|scope| {
        scope.spawn(|| {
            let big = budget.reserve(5).unwrap();
            order.lock().push("big");
            drop(big);
        });
        wait_for(&budget, 1);
        scope.spawn(|| {
            let small = budget.reserve(1).unwrap();
            order.lock().push("small");
            drop(small);
        });
        wait_for(&budget, 2);
        assert_eq!(budget.available(), 2);
        assert!(order.lock().is_empty());
        drop(head);
    });
    assert_eq!(*order.lock(), ["big", "small"]);
    assert_eq!(budget.available(), 10);
}

#[test]
fn concurrent_usage_never_exceeds_capacity() {
    let budget = Budget::new(Level::Work, 7);
    let used = AtomicU64::new(0);
    let peak = AtomicU64::new(0);
    thread::scope(|scope| {
        for worker in 0..8_u64 {
            let (budget, used, peak) = (&budget, &used, &peak);
            scope.spawn(move || {
                for round in 0..200_u64 {
                    let bytes = (worker + round) % 5 + 1;
                    let held = budget.reserve(bytes).unwrap();
                    let now = used.fetch_add(bytes, Ordering::SeqCst) + bytes;
                    peak.fetch_max(now, Ordering::SeqCst);
                    used.fetch_sub(bytes, Ordering::SeqCst);
                    drop(held);
                }
            });
        }
    });
    assert!(peak.load(Ordering::SeqCst) <= 7);
    assert_eq!(budget.available(), 7);
}

#[test]
fn panic_while_holding_releases_the_reservation() {
    let budget = Budget::new(Level::Work, 4);
    let result = thread::scope(|scope| {
        scope
            .spawn(|| {
                let _held = budget.reserve(4).unwrap();
                panic!("inject");
            })
            .join()
    });
    assert!(result.is_err());
    assert_eq!(budget.available(), 4);
}

#[test]
#[should_panic(expected = "level order violated")]
fn reserving_same_level_twice_on_one_thread_panics() {
    let budget = Budget::new(Level::Work, 4);
    let _first = budget.reserve(1).unwrap();
    let _second = budget.reserve(1);
}

#[test]
#[should_panic(expected = "level order violated")]
fn reserving_hold_while_holding_work_panics() {
    let work = Budget::new(Level::Work, 4);
    let hold = Budget::new(Level::Hold, 4);
    let _work = work.reserve(1).unwrap();
    let _hold = hold.reserve(1);
}

#[test]
fn ledger_charges_until_capacity_then_reports_need() {
    let ledger = Ledger::new(10);
    assert_eq!(ledger.try_charge(6), Ok(()));
    assert_eq!(
        ledger.try_charge(5),
        Err(CapacityExceeded {
            need: 11,
            capacity: 10
        })
    );
    assert_eq!(ledger.try_charge(4), Ok(()));
    assert!(ledger.try_charge(u64::MAX).is_err());
    ledger.reset();
    assert_eq!(ledger.try_charge(10), Ok(()));
}
