use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::compute_output_prefix;
use super::decide_protect;
use super::find_duplicates;
use super::render_script;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::file_index::DuplicateGroup;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;

fn local_data_dir() -> (Location, Arc<dyn Backend>) {
    (
        Location::Local(Utf8PathBuf::from(tc::DATA_DIR)),
        LocalBackend::arc(),
    )
}

fn local_dir(p: &std::path::Path) -> (Location, Arc<dyn Backend>) {
    (
        Location::Local(Utf8PathBuf::from(p.to_str().unwrap())),
        LocalBackend::arc(),
    )
}

fn run_render(same: &[DuplicateGroup], prefix: Option<&str>) -> String {
    let mut sink: Vec<u8> = Vec::new();
    render_script(same, prefix, &mut sink);
    String::from_utf8(sink).unwrap()
}

// sample 已按 size 降序（render_script 不再 iter().rev()，调用方负责排序）。
fn sample_two_groups() -> Vec<DuplicateGroup> {
    vec![
        DuplicateGroup {
            size: 200,
            paths: vec![
                Utf8PathBuf::from("/data/big_a"),
                Utf8PathBuf::from("/data/big_b"),
            ],
        },
        DuplicateGroup {
            size: 100,
            paths: vec![
                Utf8PathBuf::from("/data/small_a"),
                Utf8PathBuf::from("/data/small_b"),
            ],
        },
    ]
}

include!("find_part1_tests.rs");
include!("find_part2_tests.rs");
