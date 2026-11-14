use super::*;

const MIB: u64 = 1 << 20;

fn input(cpus: usize) -> PlanInput {
    PlanInput {
        limit: 2048 * MIB,
        reserve: 256 * MIB,
        baseline: 96 * MIB,
        model: 256 * MIB,
        per_thread: 4 * MIB,
        hold: 128 * MIB,
        work: 384 * MIB,
        io: IoSizing::DEFAULT,
        cpus,
    }
}

#[test]
fn io_threads_scale_with_cpus_and_clamp() {
    assert_eq!(IoSizing::DEFAULT.threads(1), 8);
    assert_eq!(IoSizing::DEFAULT.threads(10), 40);
    assert_eq!(IoSizing::DEFAULT.threads(64), 64);
    assert_eq!(IoSizing::DEFAULT.threads(usize::MAX), 64);
}

#[test]
fn io_sizing_rejects_zero_and_inverted_bounds() {
    assert!(IoSizing::DEFAULT.is_valid());
    assert!(
        !IoSizing {
            per_cpu: 0,
            ..IoSizing::DEFAULT
        }
        .is_valid()
    );
    assert!(
        !IoSizing {
            min: 0,
            ..IoSizing::DEFAULT
        }
        .is_valid()
    );
    assert!(
        !IoSizing {
            min: 9,
            max: 8,
            per_cpu: 1
        }
        .is_valid()
    );
}

#[test]
fn derive_gives_remainder_to_retained() {
    let plan = derive(&input(10));
    let threads = 40 + 10 + 4;
    assert_eq!(plan.hold, 128 * MIB);
    assert_eq!(plan.work, 384 * MIB);
    assert_eq!(
        plan.retained,
        (2048 - 256 - 96 - 256 - 128 - 384 - 4 * threads) * MIB
    );
}

#[test]
fn derive_saturates_to_zero_when_fixed_costs_exceed_limit() {
    let mut tight = input(10);
    tight.limit = 512 * MIB;
    assert_eq!(derive(&tight).retained, 0);
    tight.per_thread = u64::MAX;
    assert_eq!(derive(&tight).retained, 0);
}
