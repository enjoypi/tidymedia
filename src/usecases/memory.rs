use std::sync::OnceLock;

use crate::entities::memory::{Governor, Ledger, derive};
use crate::entities::threadpool;
use crate::usecases::config::config;
use crate::usecases::retained::EntryCost;

static GOVERNOR: OnceLock<Governor> = OnceLock::new();

pub(crate) fn governor() -> &'static Governor {
    GOVERNOR.get_or_init(|| {
        let memory = &config().memory;
        Governor::new(derive(&memory.plan_input(threadpool::cpus())))
    })
}

pub(crate) fn install() {
    threadpool::configure_io(config().memory.io_sizing());
    crate::entities::backend::set_read_buffer_bytes(config().memory.read_buffer_bytes);
    let memory = &config().memory;
    crate::entities::exif::set_parse_limits(crate::entities::exif::ParseLimits {
        work: &governor().work,
        free_bytes: memory.metadata_free_bytes,
        max_bytes: memory.metadata_max_bytes,
    });
}

#[cfg(test)]
thread_local! {
    static RETAINED_OVERRIDE: std::cell::Cell<Option<&'static Ledger>> =
        const { std::cell::Cell::new(None) };
}

pub(crate) fn retained() -> &'static Ledger {
    #[cfg(test)]
    if let Some(ledger) = RETAINED_OVERRIDE.get() {
        return ledger;
    }
    &governor().retained
}

#[cfg(test)]
pub(crate) fn with_retained<R>(capacity: u64, f: impl FnOnce() -> R) -> R {
    let ledger: &'static Ledger = Box::leak(Box::new(Ledger::new(capacity)));
    RETAINED_OVERRIDE.set(Some(ledger));
    let result = f();
    RETAINED_OVERRIDE.set(None);
    result
}

fn cost(base: u64) -> EntryCost {
    EntryCost {
        base,
        path_copies: config().memory.path_copies,
    }
}

pub(crate) fn source_cost() -> EntryCost {
    cost(config().memory.source_entry_bytes)
}

pub(crate) fn output_cost() -> EntryCost {
    cost(config().memory.output_entry_bytes)
}

pub(crate) fn name_cost() -> EntryCost {
    EntryCost {
        base: config().memory.name_entry_bytes,
        path_copies: 0,
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
