use super::{io_pool, install_io};

// io_pool 单例：多次调用返回同一 ThreadPool 引用（OnceLock 语义）。
#[test]
fn io_pool_returns_singleton_instance() {
    let p1: *const _ = io_pool();
    let p2: *const _ = io_pool();
    assert_eq!(p1, p2, "io_pool must be OnceLock-backed singleton");
}

// install_io 在 I/O 池上跑闭包并透传返值；用于钉契约：caller 不必关心
// rayon::ThreadPool 类型，只需 install_io(|| par_iter...) 包裹。
#[test]
fn install_io_returns_closure_result() {
    let result: u32 = install_io(|| 42);
    assert_eq!(result, 42);
}

// 默认线程数在 [IO_THREADS_MIN, IO_THREADS_MAX] = [8, 64] 范围内：保证「CPU × 4
// clamp 公式」不退化为意外值。当前机器 CPU 数未知，断言 range 而非具体值。
#[test]
fn io_pool_default_thread_count_within_bounds() {
    let n = io_pool().current_num_threads();
    assert!(
        (8..=64).contains(&n),
        "default I/O pool size {n} must be within [8, 64]"
    );
}

// install_io 在嵌套调用（pool 内再调一次）下仍正常工作；防回归 rayon
// thread-local 状态被错误重入的边界。
#[test]
fn install_io_supports_nested_invocation() {
    let outer: u32 = install_io(|| {
        let inner: u32 = install_io(|| 7);
        inner * 3
    });
    assert_eq!(outer, 21);
}
