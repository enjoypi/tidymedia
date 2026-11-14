#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoSizing {
    pub per_cpu: usize,
    pub min: usize,
    pub max: usize,
}

impl IoSizing {
    pub const DEFAULT: Self = Self {
        per_cpu: 4,
        min: 8,
        max: 64,
    };

    #[must_use]
    pub fn threads(&self, cpus: usize) -> usize {
        cpus.saturating_mul(self.per_cpu).clamp(self.min, self.max)
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.per_cpu == 0 || self.min == 0 {
            return false;
        }
        self.min <= self.max
    }
}

const AUXILIARY_THREADS: u64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanInput {
    pub limit: u64,
    pub reserve: u64,
    pub baseline: u64,
    pub model: u64,
    pub per_thread: u64,
    pub hold: u64,
    pub work: u64,
    pub io: IoSizing,
    pub cpus: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryPlan {
    pub hold: u64,
    pub work: u64,
    pub retained: u64,
}

#[must_use]
pub fn derive(input: &PlanInput) -> MemoryPlan {
    let threads = (input.io.threads(input.cpus) as u64)
        .saturating_add(input.cpus as u64)
        .saturating_add(AUXILIARY_THREADS);
    let fixed = [
        input.reserve,
        input.baseline,
        input.model,
        input.per_thread.saturating_mul(threads),
        input.hold,
        input.work,
    ]
    .into_iter()
    .fold(0_u64, u64::saturating_add);
    MemoryPlan {
        hold: input.hold,
        work: input.work,
        retained: input.limit.saturating_sub(fixed),
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
