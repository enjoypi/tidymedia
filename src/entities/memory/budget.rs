use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::{Condvar, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Hold = 1,
    Work = 2,
}

thread_local! {
    static HELD: Cell<u8> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge {
    pub need: u64,
    pub capacity: u64,
}

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "needs {} bytes, memory budget is {} bytes",
            self.need, self.capacity
        )
    }
}

struct State {
    available: u64,
    next_ticket: u64,
    serving: u64,
}

pub struct Budget {
    level: Level,
    capacity: u64,
    state: Mutex<State>,
    freed: Condvar,
}

pub struct Reservation<'a> {
    budget: &'a Budget,
    bytes: u64,
    thread_bound: PhantomData<*const ()>,
}

impl Budget {
    #[must_use]
    pub const fn new(level: Level, capacity: u64) -> Self {
        Self {
            level,
            capacity,
            state: Mutex::new(State {
                available: capacity,
                next_ticket: 0,
                serving: 0,
            }),
            freed: Condvar::new(),
        }
    }

    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    pub fn reserve(&self, bytes: u64) -> Result<Reservation<'_>, TooLarge> {
        if bytes > self.capacity {
            return Err(TooLarge {
                need: bytes,
                capacity: self.capacity,
            });
        }
        let bit = self.level as u8;
        let held = HELD.get();
        assert!(held < bit, "internal: memory budget level order violated");
        let mut state = self.state.lock();
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        while !ready(&state, ticket, bytes) {
            self.freed.wait(&mut state);
        }
        state.available -= bytes;
        state.serving += 1;
        drop(state);
        self.freed.notify_all();
        HELD.set(held | bit);
        Ok(Reservation {
            budget: self,
            bytes,
            thread_bound: PhantomData,
        })
    }

    #[cfg(test)]
    pub(super) fn waiting(&self) -> u64 {
        let state = self.state.lock();
        state.next_ticket - state.serving
    }

    #[cfg(test)]
    pub(super) fn available(&self) -> u64 {
        self.state.lock().available
    }
}

fn ready(state: &State, ticket: u64, bytes: u64) -> bool {
    if state.serving != ticket {
        return false;
    }
    state.available >= bytes
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.budget.state.lock().available += self.bytes;
        self.budget.freed.notify_all();
        HELD.set(HELD.get() & !(self.budget.level as u8));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapacityExceeded {
    pub need: u64,
    pub capacity: u64,
}

pub struct Ledger {
    capacity: u64,
    used: AtomicU64,
}

impl Ledger {
    #[must_use]
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity,
            used: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    pub fn try_charge(&self, bytes: u64) -> Result<(), CapacityExceeded> {
        self.used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes)
                    .filter(|next| *next <= self.capacity)
            })
            .map(drop)
            .map_err(|used| CapacityExceeded {
                need: used.saturating_add(bytes),
                capacity: self.capacity,
            })
    }

    pub fn reset(&self) {
        self.used.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
