use super::{governor, install};
use crate::entities::memory::derive;
use crate::entities::threadpool;
use crate::usecases::config::config;

#[test]
fn governor_follows_configured_plan_and_is_a_singleton() {
    install();
    let plan = derive(&config().memory.plan_input(threadpool::cpus()));
    let first = governor();
    assert_eq!(first.hold.capacity(), plan.hold);
    assert_eq!(first.work.capacity(), plan.work);
    assert_eq!(first.retained.capacity(), plan.retained);
    assert!(std::ptr::eq(first, governor()));
    assert_eq!(
        crate::entities::backend::read_buffer_bytes(),
        config().memory.read_buffer_bytes
    );
}
