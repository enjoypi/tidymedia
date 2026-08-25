//! F1 分桶并行复制循环：按 `fast_hash` 分桶（桶内 `full_path` 字典序保序）→ 桶间
//! `par_iter` 并行 → 每桶串行 `do_copy` → `CopyDelta` 树归并汇总。

use std::collections::BTreeMap;
use std::sync::Arc;

use camino::Utf8PathBuf;
use dashmap::DashSet;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tracing::error;

use super::ops::do_copy;
use super::run::CopyOpts;
use crate::entities::backend::Backend;
use crate::entities::file_index::{Index, VisitStats};
use crate::entities::threadpool::install_io;
use crate::entities::uri::Location;
use crate::usecases::report::{ReportError, extend_errors_capped, feature_of, push_error_capped};

// F1 分桶并行：按 fast_hash 分桶（BTreeMap 保序）+ 桶内 full_path 字典序，让同
// hash 组内 winner 顺序在并行下仍可重现；桶间 par_iter 并行执行，桶内串行处理
// 让 do_copy 的 output_index.exists → add 序列在每桶内保持原有 dedup 语义。
pub(super) fn run_copy_loop(
    source: &Index,
    output_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    opts: &CopyOpts<'_>,
) -> (usize, usize, usize, Vec<ReportError>, bool) {
    // root 不存在（dry-run 首次归档最常见；非 dry-run 已 mkdir_p 必存在）→ 整树
    // 为空，跳过 walk。exists 的 Err 不吞成「不存在」：保守按存在处理走 walk，
    // walk 失败会计 walker_errors 让 index_authoritative=false，后续逐文件
    // exists 探测时错误自然传播。
    let walk_output = !matches!(output_backend.exists(output_loc), Ok(false));
    let mut output_index = Index::new();
    if walk_output {
        output_index.visit_location(output_loc, output_backend);
    }
    // output 扫描零 skip / 零 walker 错误 ⇒ 索引是 output 的完整快照（权威），
    // generate_unique_name 可跳过逐文件 backend.exists 探测（远端每文件一次 RTT）。
    // 有任何 skip（空文件 / 不可读 / walker 错误）⇒ 磁盘上存在索引外文件，保留探测。
    let index_authoritative = output_index.stats() == VisitStats::default();
    // freeze 成 shared &Index：Index 内 DashMap 让 add / exists / contains_target 均
    // &self 语义，跨 par_iter 边界安全并发。
    let output_index = output_index;

    // 提出宏外：tracing 字段表达式仅在事件被订阅时求值，留在宏内会成为
    // 测试中永不执行的 region，破坏 100% 覆盖率口径。
    let feature = feature_of(opts.remove);

    // mkdir 缓存：同 {year}/{month} 桶下所有文件共享一次 mkdir_p，远端 backend
    // 单 source 万文件分布 12 个月份只触发 12 次 mkdir_recursive 而非 10000 次。
    // 仅命中已成功路径；mkdir_p 失败的 dir 不入缓存让下次仍尝试创建（避开
    // 「首次失败永驻 false-positive」陷阱）。DashSet 让跨 par_iter 边界并发 contains
    // + insert，双 worker 撞同桶最坏多一次 mkdir_p RTT，mkdir_p 幂等可接受。
    let mkdir_cache: DashSet<Location> = DashSet::new();

    // Phase A：按 fast_hash 分桶 + 桶内 full_path 字典序。同 fast_hash 组内的 winner
    // 顺序（谁先入 output_index → 后续同 hash src 命中 exists → ignored）由此串行序
    // 决定，跨桶无 hash 交互可安全并行。BTreeMap 遍历顺序按 u64 升序确定，让
    // groups 索引可重现。
    let mut by_hash: BTreeMap<u64, Vec<Utf8PathBuf>> = BTreeMap::new();
    for kv in source.iter() {
        by_hash
            .entry(kv.value().fast_hash)
            .or_default()
            .push(kv.key().clone());
    }
    for g in by_hash.values_mut() {
        g.sort();
    }
    let groups: Vec<Vec<Utf8PathBuf>> = by_hash.into_values().collect();

    // Phase B：桶间 par_iter 并行；每桶内串行处理，reduce 汇总 CopyDelta。
    // install_io 让循环走 I/O 池（CPU×4 clamp[8,64]）；do_copy 内的远端 open_read /
    // mkdir_p / rename 是同步阻塞 IO，走全局 rayon 池会挤占 CPU-bound 阶段。
    let delta = install_io(|| {
        groups
            .par_iter()
            .map(|grp| {
                process_group(
                    grp,
                    source,
                    &output_index,
                    &mkdir_cache,
                    index_authoritative,
                    opts,
                    output_loc,
                    output_backend,
                    feature,
                )
            })
            .reduce(CopyDelta::default, CopyDelta::merge)
    });

    (
        delta.copied,
        delta.ignored,
        delta.failed,
        delta.errors,
        delta.errors_truncated,
    )
}

/// 单个 `fast_hash` 桶内的串行处理。桶间调用者并行，桶内保持字典序遍历让
/// dedup 语义（先入者留，后入者 ignored）确定性。
#[expect(
    clippy::too_many_arguments,
    reason = "桶内串行需要 caller 的全部并行上下文透传；折结构体反让 par_iter 闭包更绕"
)]
fn process_group(
    grp: &[Utf8PathBuf],
    source: &Index,
    output_index: &Index,
    mkdir_cache: &DashSet<Location>,
    index_authoritative: bool,
    opts: &CopyOpts<'_>,
    output_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    feature: &'static str,
) -> CopyDelta {
    let mut local = CopyDelta::default();
    for key in grp {
        // grp 内 key 来自本次 build_source_index 后 source.iter() 的 snapshot；
        // 本 fn 运行期间无人 remove，`.get()` 必 Some。若真 None 表示 Index 状态破坏，
        // 内部 bug 直接 panic 让上游可查（CLAUDE.md「不可达用 `.expect("internal: ...")`」）。
        let kv = source
            .get(key.as_path())
            .expect("internal: fast_hash group key must resolve in source snapshot");
        let src = kv.value();
        match do_copy(
            src,
            output_loc,
            output_backend,
            output_index,
            mkdir_cache,
            index_authoritative,
            opts,
        ) {
            Ok(true) => local.copied += 1,
            Ok(false) => local.ignored += 1,
            Err(e) => {
                local.failed += 1;
                let msg = e.to_string();
                error!(
                    feature,
                    operation = "do_copy",
                    result = "error",
                    source = %src.full_path,
                    dry_run = opts.dry_run,
                    remove = opts.remove,
                    error = %msg,
                    "copy item failed"
                );
                push_error_capped(
                    &mut local.errors,
                    &mut local.errors_truncated,
                    ReportError {
                        path: src.full_path.to_string(),
                        message: msg,
                    },
                );
            }
        }
    }
    local
}

/// `par_iter` map-reduce 汇总项：每 worker 局部累加避免全局 `Mutex` 串行化；
/// rayon tree-reduce 归并到根，`errors` 经 [`extend_errors_capped`] 受 soft cap 保护。
#[derive(Default)]
struct CopyDelta {
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
    errors_truncated: bool,
}

impl CopyDelta {
    fn merge(mut a: Self, b: Self) -> Self {
        a.copied += b.copied;
        a.ignored += b.ignored;
        a.failed += b.failed;
        extend_errors_capped(
            &mut a.errors,
            &mut a.errors_truncated,
            b.errors,
            b.errors_truncated,
        );
        a
    }
}
