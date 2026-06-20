//! I/O-bound 工作专用 rayon 线程池：远端 backend 的 read/stat 在 `par_iter` 内
//! 同步阻塞，全局 rayon 池线程数 = CPU 核数（如 8）会让 N 个文件下载占满全部
//! CPU 线程，CPU-bound 后续阶段（pHash / EXIF 解析）无线程可用。
//!
//! 本 pool 默认 CPU × 4，clamp `[8, 64]`，仅服务 I/O-bound 阶段
//! （`visit_location` / `parse_exif` / `enrich_candidates`），cull 等 CPU-heavy
//! 路径继续走 rayon 全局池。OS 线程创建失败（如 ulimit nproc 耗尽）→ panic：
//! 该环境下全局池同样无法构造，没有合理 fallback，让 user 看到资源耗尽根因
//! 比静默退化到串行更可诊断。

use std::num::NonZeroUsize;
use std::sync::OnceLock;
use std::thread::available_parallelism;

use rayon::ThreadPool;

static IO_POOL: OnceLock<ThreadPool> = OnceLock::new();

const IO_THREADS_MIN: usize = 8;
const IO_THREADS_MAX: usize = 64;
const IO_THREADS_PER_CPU: usize = 4;

/// 取 I/O 专用线程池；首次访问时按 `CPU × 4` 并 clamp 到 `[8, 64]` 构造。
/// 调用方一般用 [`install_io`] wrapper 而非直接持 ref。
#[must_use]
pub fn io_pool() -> &'static ThreadPool {
    IO_POOL.get_or_init(build_pool)
}

/// 在 I/O 专用池上跑闭包；caller 通常包 `par_iter().for_each(...)`。
/// 等价于 `io_pool().install(f)`，多一层 wrapper 让调用点不直接持 pool ref。
pub fn install_io<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    io_pool().install(f)
}

fn build_pool() -> ThreadPool {
    let cpus = available_parallelism().map_or(4, NonZeroUsize::get);
    let threads = (cpus * IO_THREADS_PER_CPU).clamp(IO_THREADS_MIN, IO_THREADS_MAX);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("tm-io-{i}"))
        .build()
        // ThreadPoolBuilder::build 仅在 OS spawn 失败返 Err（ulimit nproc 耗尽 /
        // 内存压力下 spawn 失败等）；该环境下 rayon 全局池同样无法构造，没有合理
        // fallback。Panic 让 user 立即看到资源耗尽根因，避免静默退化到串行让
        // 性能问题更难诊断（与 OnceLock::get_or_init 失败语义对齐）。
        .expect("internal: failed to build I/O thread pool")
}

#[cfg(test)]
#[path = "threadpool_tests.rs"]
mod tests;
