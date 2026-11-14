use tracing::warn;

use super::eprintln_sanitize_fallback;
use crate::entities::memory::derive;
use crate::usecases::config::{Config, MemoryConfig};

const LIMIT_MIN_BYTES: u64 = 512 << 20;
const IO_THREADS_CEILING: usize = 64;
const DECODE_FACTOR_CEILING: u64 = 16;
const METADATA_GROWTH_FACTOR: u64 = 3;
const WORK_MIN_BYTES: u64 = 16 << 20;
const _: () = assert!(WORK_MIN_BYTES >= crate::usecases::copy::EXTRACT_WORK_BYTES);
const READ_BUFFER_RANGE: std::ops::RangeInclusive<usize> = (4 << 10)..=(1 << 20);
const RESIZE_MAX_SIDE_CEILING: u32 = 1024;
const MAX_TEXT_BYTES_CEILING: usize = 1 << 20;
const MAX_TSV_BYTES_CEILING: u64 = 256 << 20;

pub(super) fn sanitize_memory(cfg: &mut Config, cpus: usize) {
    if !is_feasible(&cfg.memory, cpus) {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "memory",
            "memory plan is infeasible; falling back to default"
        );
        eprintln_sanitize_fallback("memory", "infeasible", &"defaults");
        cfg.memory = MemoryConfig::default();
    }
    let image_cap = cfg.memory.hold_bytes / 2;
    cap_u64(
        &mut cfg.verify.max_image_bytes,
        image_cap,
        "verify.max_image_bytes",
    );
    cap_u64(
        &mut cfg.backend.ocr.max_image_bytes,
        image_cap,
        "backend.ocr.max_image_bytes",
    );
    cap_u64(
        &mut cfg.verify.max_tsv_bytes,
        MAX_TSV_BYTES_CEILING,
        "verify.max_tsv_bytes",
    );
    let mut side = u64::from(cfg.backend.ocr.resize_max_side);
    cap_u64(
        &mut side,
        u64::from(RESIZE_MAX_SIDE_CEILING),
        "backend.ocr.resize_max_side",
    );
    cfg.backend.ocr.resize_max_side = side.try_into().unwrap_or(RESIZE_MAX_SIDE_CEILING);
    let mut text = cfg.backend.classify.max_text_bytes as u64;
    cap_u64(
        &mut text,
        MAX_TEXT_BYTES_CEILING as u64,
        "backend.classify.max_text_bytes",
    );
    cfg.backend.classify.max_text_bytes = text.try_into().unwrap_or(MAX_TEXT_BYTES_CEILING);
}

pub(super) fn is_feasible(memory: &MemoryConfig, cpus: usize) -> bool {
    if memory.limit_bytes < LIMIT_MIN_BYTES {
        return false;
    }
    if memory.decode_factor == 0 || memory.decode_factor > DECODE_FACTOR_CEILING {
        return false;
    }
    if !READ_BUFFER_RANGE.contains(&memory.read_buffer_bytes) {
        return false;
    }
    if memory.work_bytes < WORK_MIN_BYTES {
        return false;
    }
    if memory.metadata_free_bytes > memory.metadata_max_bytes
        || memory
            .metadata_max_bytes
            .saturating_mul(METADATA_GROWTH_FACTOR)
            > memory.work_bytes
    {
        return false;
    }
    let io = memory.io_sizing();
    if !io.is_valid() || io.max > IO_THREADS_CEILING {
        return false;
    }
    derive(&memory.plan_input(cpus)).retained >= memory.retained_min_bytes
}

fn cap_u64(value: &mut u64, cap: u64, field: &str) {
    if *value > cap {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "capped",
            field,
            value = *value,
            cap,
            "value exceeds the memory plan; capping"
        );
        *value = cap;
    }
}

#[cfg(test)]
#[path = "config_sanitize_memory_tests.rs"]
mod tests;
