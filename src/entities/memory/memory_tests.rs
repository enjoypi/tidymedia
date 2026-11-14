use super::*;

#[test]
fn governor_takes_capacities_from_plan() {
    let governor = Governor::new(MemoryPlan {
        hold: 1,
        work: 2,
        retained: 3,
    });
    assert_eq!(governor.hold.capacity(), 1);
    assert_eq!(governor.work.capacity(), 2);
    assert_eq!(governor.retained.capacity(), 3);
}

#[test]
fn hold_then_work_is_the_allowed_order() {
    let governor = Governor::new(MemoryPlan {
        hold: 4,
        work: 4,
        retained: 0,
    });
    let hold = governor.hold.reserve(4).unwrap();
    let work = governor.work.reserve(4).unwrap();
    drop(work);
    drop(hold);
    assert!(governor.work.reserve(1).is_ok());
}
