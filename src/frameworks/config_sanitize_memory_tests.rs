use super::{is_feasible, sanitize_memory};
use crate::usecases::config::{Config, MemoryConfig};

const MIB: u64 = 1 << 20;

#[test]
fn default_memory_plan_is_feasible_across_core_counts() {
    for cpus in [1, 10, 64] {
        assert!(is_feasible(&MemoryConfig::default(), cpus));
    }
}

#[test]
fn infeasible_plans_are_rejected() {
    let low_limit = MemoryConfig {
        limit_bytes: 511 * MIB,
        ..MemoryConfig::default()
    };
    let zero_threads = MemoryConfig {
        io_threads_min: 0,
        ..MemoryConfig::default()
    };
    let too_many_threads = MemoryConfig {
        io_threads_max: 65,
        ..MemoryConfig::default()
    };
    let zero_factor = MemoryConfig {
        decode_factor: 0,
        ..MemoryConfig::default()
    };
    let huge_factor = MemoryConfig {
        decode_factor: 17,
        ..MemoryConfig::default()
    };
    let tiny_buffer = MemoryConfig {
        read_buffer_bytes: 1024,
        ..MemoryConfig::default()
    };
    let inverted_metadata = MemoryConfig {
        metadata_free_bytes: 65 * MIB,
        ..MemoryConfig::default()
    };
    let oversized_metadata = MemoryConfig {
        metadata_max_bytes: 200 * MIB,
        ..MemoryConfig::default()
    };
    let tiny_work = MemoryConfig {
        work_bytes: 15 * MIB,
        metadata_max_bytes: MIB,
        ..MemoryConfig::default()
    };
    let no_room = MemoryConfig {
        work_bytes: 1500 * MIB,
        ..MemoryConfig::default()
    };
    for memory in [
        low_limit,
        zero_threads,
        too_many_threads,
        zero_factor,
        huge_factor,
        tiny_buffer,
        inverted_metadata,
        oversized_metadata,
        tiny_work,
        no_room,
    ] {
        assert!(!is_feasible(&memory, 10));
    }
}

#[test]
fn sanitize_replaces_infeasible_memory_with_defaults() {
    let mut cfg = Config::default();
    cfg.memory.limit_bytes = MIB;
    sanitize_memory(&mut cfg, 10);
    assert_eq!(cfg.memory, MemoryConfig::default());
}

#[test]
fn sanitize_caps_fields_that_scale_memory() {
    let mut cfg = Config::default();
    cfg.verify.max_image_bytes = 2048 * MIB;
    cfg.backend.ocr.max_image_bytes = 65 * MIB;
    cfg.backend.ocr.resize_max_side = 8000;
    cfg.backend.classify.max_text_bytes = 64 << 20;
    cfg.verify.max_tsv_bytes = 1 << 40;
    sanitize_memory(&mut cfg, 10);
    assert_eq!(cfg.verify.max_tsv_bytes, 256 * MIB);
    assert_eq!(cfg.verify.max_image_bytes, 64 * MIB);
    assert_eq!(cfg.backend.ocr.max_image_bytes, 64 * MIB);
    assert_eq!(cfg.backend.ocr.resize_max_side, 1024);
    assert_eq!(cfg.backend.classify.max_text_bytes, 1 << 20);
}

#[test]
fn sanitize_keeps_values_within_the_plan() {
    let mut cfg = Config::default();
    let before = cfg.clone();
    sanitize_memory(&mut cfg, 10);
    assert_eq!(cfg.memory, before.memory);
    assert_eq!(cfg.verify.max_image_bytes, before.verify.max_image_bytes);
    assert_eq!(
        cfg.backend.ocr.resize_max_side,
        before.backend.ocr.resize_max_side
    );
    assert_eq!(
        cfg.backend.classify.max_text_bytes,
        before.backend.classify.max_text_bytes
    );
}
