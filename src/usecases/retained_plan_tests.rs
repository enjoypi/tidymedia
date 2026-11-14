use std::sync::Arc;

use camino::Utf8PathBuf;

use super::*;
use crate::adapters::backend::fake::FakeBackend;
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::file_index::Index;
use crate::entities::uri::Location;

const COST: EntryCost = EntryCost {
    base: 100,
    path_copies: 0,
};
const NO_NAMES: EntryCost = EntryCost {
    base: 0,
    path_copies: 0,
};

fn loc(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

fn backend(files: &[(&str, Vec<u8>)]) -> Arc<dyn Backend> {
    let fake = FakeBackend::new("local");
    fake.add_dir(loc("/src"));
    for (name, data) in files {
        fake.add_file(loc(&format!("/src/{name}")), data.clone());
    }
    Arc::new(fake)
}

fn plan(be: &Arc<dyn Backend>, capacity: u64) -> Result<Planned, CapacityExceeded> {
    plan_groups(capacity, NO_NAMES, &|make| {
        let mut index = Index::new().with_admission(make(COST));
        index.visit_location(&loc("/src"), be);
        index.stats()
    })
}

fn members(be: &Arc<dyn Backend>, planned: &Planned, group: u32) -> Vec<String> {
    let mut index = Index::new().with_admission(planned.plan.filter(group));
    index.visit_location(&loc("/src"), be);
    let mut names: Vec<String> = index.iter().map(|kv| kv.key().to_string()).collect();
    names.sort();
    names
}

fn colliding_size(base: u64) -> u64 {
    (base + 1..base + (1 << 24))
        .find(|size| bucket_of(*size) == bucket_of(base))
        .expect("internal: some size shares the bucket")
}

#[test]
fn small_inputs_fit_in_one_group() {
    let be = backend(&[("a", vec![1; 10]), ("b", vec![2; 20])]);
    let planned = plan(&be, 1_000).unwrap();
    assert_eq!(planned.plan.groups, 1);
    assert_eq!(members(&be, &planned, 0).len(), 2);
}

#[test]
fn oversized_bucket_splits_by_exact_size() {
    let other = usize::try_from(colliding_size(10)).unwrap();
    let be = backend(&[
        ("a", vec![1; 10]),
        ("b", vec![2; 10]),
        ("c", vec![3; other]),
    ]);
    let planned = plan(&be, 250).unwrap();
    assert_eq!(planned.plan.groups, 2);
    assert_eq!(members(&be, &planned, 0), ["/src/a", "/src/b"]);
    assert_eq!(members(&be, &planned, 1), ["/src/c"]);
}

#[test]
fn oversized_exact_size_splits_by_fast_hash() {
    let be = backend(&[("a", vec![1; 10]), ("b", vec![2; 10]), ("c", vec![3; 10])]);
    let planned = plan(&be, 150).unwrap();
    assert_eq!(planned.plan.groups, 3);
    let all: usize = (0..3).map(|g| members(&be, &planned, g).len()).sum();
    assert_eq!(all, 3);
}

#[test]
fn identical_files_beyond_capacity_split_by_path() {
    let be = backend(&[("a", vec![7; 10]), ("b", vec![7; 10]), ("c", vec![7; 10])]);
    let planned = plan(&be, 150).unwrap();
    let mut seen = Vec::new();
    for group in 0..planned.plan.groups {
        let mine = members(&be, &planned, group);
        assert!(mine.len() <= 1);
        seen.extend(mine);
        let mut outputs = Index::new().with_admission(planned.plan.output_filter(group));
        outputs.visit_location(&loc("/src"), &be);
        assert!(outputs.len() == 3 || outputs.is_empty());
    }
    seen.sort();
    assert_eq!(seen, ["/src/a", "/src/b", "/src/c"]);
}

#[test]
fn single_entry_larger_than_capacity_is_rejected() {
    let be = backend(&[("a", vec![7; 10])]);
    assert_eq!(plan(&be, 50).err().unwrap().need, 100);
}

#[test]
fn unknown_size_in_oversized_bucket_falls_back_to_first_group() {
    let mut bucket_group = vec![0; 1 << 16];
    bucket_group[bucket_of(10)] = OVERSIZED;
    let plan = Arc::new(Plan {
        bucket_group,
        exact: HashMap::new(),
        split: HashMap::new(),
        groups_of_size: HashMap::new(),
        groups: 1,
    });
    let entry = Entry {
        location: loc("/x"),
        size: 10,
        kind: EntryKind::File,
    };
    assert!(plan.filter(0).owns_entry(&entry));
    assert!(!plan.filter(1).owns_entry(&entry));
}
