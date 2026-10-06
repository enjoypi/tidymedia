use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::{dedupe_or_pick_target, drain_and_hash_equal};
use crate::adapters::backend::fake::{FakeBackend, Op};
use crate::entities::backend::Backend;
use crate::entities::test_common::FlakyIo;
use crate::entities::uri::Location;

fn local(p: &str) -> Location {
    Location::Local(Utf8PathBuf::from(p))
}

fn fake_with_existing_target() -> Arc<FakeBackend> {
    let fake = Arc::new(FakeBackend::new("local"));
    fake.add_dir(local("/out"));
    fake.add_file(local("/out/shot.png"), b"existing".to_vec());
    fake
}

#[test]
fn drain_and_hash_equal_propagates_read_error() {
    let mut io = FlakyIo::new(vec![1], 0, 0);
    assert!(drain_and_hash_equal(&mut io, b"x").is_err());
}

#[test]
fn dedupe_propagates_metadata_error() {
    let fake = fake_with_existing_target();
    fake.inject_error(
        local("/out/shot.png"),
        Op::Metadata,
        io::ErrorKind::TimedOut,
    );
    let be: Arc<dyn Backend> = fake;
    let err = dedupe_or_pick_target(
        &local("/out/shot.png"),
        "shot.png",
        &local("/out"),
        &be,
        b"new",
    )
    .err()
    .expect("dedupe must fail");
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn dedupe_propagates_unique_name_error() {
    let fake = fake_with_existing_target();
    fake.inject_error(
        local("/out/shot_1.png"),
        Op::Exists,
        io::ErrorKind::TimedOut,
    );
    let be: Arc<dyn Backend> = fake;
    let err = dedupe_or_pick_target(
        &local("/out/shot.png"),
        "shot.png",
        &local("/out"),
        &be,
        b"new",
    )
    .err()
    .expect("dedupe must fail");
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}
