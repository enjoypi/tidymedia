mod budget;
mod plan;

pub use budget::{Budget, CapacityExceeded, Ledger, Level, Reservation, TooLarge};
pub use plan::{IoSizing, MemoryPlan, PlanInput, derive};

pub struct Governor {
    pub hold: Budget,
    pub work: Budget,
    pub retained: Ledger,
}

impl Governor {
    #[must_use]
    pub fn new(plan: MemoryPlan) -> Self {
        Self {
            hold: Budget::new(Level::Hold, plan.hold),
            work: Budget::new(Level::Work, plan.work),
            retained: Ledger::new(plan.retained),
        }
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
