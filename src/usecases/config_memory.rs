use serde_derive::Deserialize;

use crate::entities::backend::DEFAULT_READ_BUFFER_BYTES;
use crate::entities::memory::{IoSizing, PlanInput};

const MIB: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryConfig {
    pub limit_bytes: u64,
    pub reserve_bytes: u64,
    pub baseline_bytes: u64,
    pub model_bytes: u64,
    pub per_thread_bytes: u64,
    pub hold_bytes: u64,
    pub work_bytes: u64,
    pub retained_min_bytes: u64,
    pub decode_factor: u64,
    pub read_buffer_bytes: usize,
    pub metadata_free_bytes: u64,
    pub metadata_max_bytes: u64,
    pub source_entry_bytes: u64,
    pub output_entry_bytes: u64,
    pub name_entry_bytes: u64,
    pub path_copies: u64,
    pub io_threads_per_cpu: usize,
    pub io_threads_min: usize,
    pub io_threads_max: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            limit_bytes: 2048 * MIB,
            reserve_bytes: 256 * MIB,
            baseline_bytes: 96 * MIB,
            model_bytes: 256 * MIB,
            per_thread_bytes: 4 * MIB,
            hold_bytes: 128 * MIB,
            work_bytes: 384 * MIB,
            retained_min_bytes: 64 * MIB,
            decode_factor: 3,
            read_buffer_bytes: DEFAULT_READ_BUFFER_BYTES,
            metadata_free_bytes: MIB,
            metadata_max_bytes: 64 * MIB,
            source_entry_bytes: 3584,
            output_entry_bytes: 2048,
            name_entry_bytes: 48,
            path_copies: 5,
            io_threads_per_cpu: IoSizing::DEFAULT.per_cpu,
            io_threads_min: IoSizing::DEFAULT.min,
            io_threads_max: IoSizing::DEFAULT.max,
        }
    }
}

impl MemoryConfig {
    #[must_use]
    pub fn io_sizing(&self) -> IoSizing {
        IoSizing {
            per_cpu: self.io_threads_per_cpu,
            min: self.io_threads_min,
            max: self.io_threads_max,
        }
    }

    #[must_use]
    pub fn plan_input(&self, cpus: usize) -> PlanInput {
        PlanInput {
            limit: self.limit_bytes,
            reserve: self.reserve_bytes,
            baseline: self.baseline_bytes,
            model: self.model_bytes,
            per_thread: self.per_thread_bytes,
            hold: self.hold_bytes,
            work: self.work_bytes,
            io: self.io_sizing(),
            cpus,
        }
    }
}
