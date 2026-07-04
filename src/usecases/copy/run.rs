//! copy 主流程编排：扫源建索引 → 解析 EXIF → 循环 `do_copy` → 汇总报告。

use std::collections::BTreeMap;
use std::sync::Arc;

use camino::Utf8PathBuf;
use chrono::FixedOffset;
use chrono::Offset;
use dashmap::DashSet;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use time::UtcOffset;
use tracing::debug;
use tracing::error;
use tracing::trace;

use super::ops::do_copy;
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::common::{canonical_prefix, under_prefix};
use crate::entities::file_index::{CandidateProvider, Index, VisitStats};
use crate::entities::threadpool::install_io;
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::report::{
    CopyReport, FEATURE_COPY, FEATURE_MOVE, Report, ReportError, ReportSink, feature_of,
};

// FEATURE_COPY / FEATURE_MOVE / feature_of 均从 usecases::report 单点导入；
// archive_template / adapters::report_sink 亦共用同一定义，避免任一漂移分裂日志聚合。
// _ 抑制未直接使用的 re-export lint（FEATURE_MOVE 只出现在 report_sink，本模块经
// feature_of 间接消费）。
const _: (&str, &str) = (FEATURE_COPY, FEATURE_MOVE);

/// usecase 入口的 source / output 对：把 [`Location`] 与负责该 scheme 的
/// [`Backend`] 句柄一起传入，避免内层重新解析 URI。
pub type Source = (Location, Arc<dyn Backend>);

pub(super) const MONTH: [&str; 13] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

/// [`do_copy`] 的选项包；把 bool + template 打包，规避 `clippy::too_many_arguments`。
pub struct CopyOpts<'a> {
    pub dry_run: bool,
    pub remove: bool,
    pub include_non_media: bool,
    pub template: &'a str,
}

pub(super) fn configured_offset() -> UtcOffset {
    offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界回退到 UTC，避免 panic；time crate 合法范围 ±25:59:59。
pub(super) fn offset_from_hours(hours: i8) -> UtcOffset {
    UtcOffset::from_whole_seconds(i32::from(hours) * 3600).unwrap_or(UtcOffset::UTC)
}

// chrono::FixedOffset 用于把 EXIF / 文件名内无时区的 NaiveDateTime 当相机本地时间
// 解释；与 time::UtcOffset 共用同一份 timezone_offset_hours 配置。
pub(super) fn configured_chrono_offset() -> FixedOffset {
    chrono_offset_from_hours(config().copy.timezone_offset_hours)
}

// 越界（chrono::FixedOffset 合法 ±86_400 秒，即 ±24h）回退到 UTC。
pub(super) fn chrono_offset_from_hours(hours: i8) -> FixedOffset {
    FixedOffset::east_opt(i32::from(hours) * 3600).unwrap_or_else(|| chrono::Utc.fix())
}

/// 测试 shim：等价于 `copy_with_sidecar(..., None)`。
/// 生产路径（dispatch）走 [`copy_with_sidecar`] 注入 P3 发现；仅测试用本简短入口。
#[cfg(test)]
pub fn copy(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report_sink: Option<&dyn ReportSink>,
) -> common::Result<CopyReport> {
    copy_with_sidecar(
        sources,
        output,
        dry_run,
        remove,
        include_non_media,
        archive_template,
        report_sink,
        None,
    )
}

// 8 个参数源于 CLI 选项的一比一透传；与 make_report 同理。
#[expect(
    clippy::too_many_arguments,
    reason = "CLI 选项 + sidecar provider 一比一透传，折结构体会让 dispatch 调用点同样冗长"
)]
pub fn copy_with_sidecar(
    sources: &[Source],
    output: Source,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    archive_template: Option<&str>,
    report_sink: Option<&dyn ReportSink>,
    sidecar: Option<CandidateProvider>,
) -> common::Result<CopyReport> {
    let (output_loc, output_backend) = output;
    let template = archive_template.unwrap_or(&config().copy.archive_template);

    let output_prefix = canonical_prefix(&output_loc);
    ensure_sources_outside_output(sources, &output_prefix)?;
    let feature = feature_of(remove);
    let source = build_source_index(sources, &output_prefix, sidecar, feature);

    let total_files = source.len();
    let scan_stats = source.stats();
    log_scan_summary(feature, total_files, scan_stats);

    let flags = ReportFlags {
        dry_run,
        remove,
        include_non_media,
    };
    if total_files == 0 {
        return Ok(finalize(
            report_sink,
            flags,
            scan_stats,
            0,
            0,
            0,
            Vec::new(),
        ));
    }

    trace!(
        feature,
        operation = "sample_files",
        sample = ?source.some_files(10),
        "first files sample"
    );

    if !dry_run {
        output_backend.mkdir_p(&output_loc)?;
    }

    let opts = CopyOpts {
        dry_run,
        remove,
        include_non_media,
        template,
    };
    let (copied, ignored, failed, errors) =
        run_copy_loop(&source, &output_loc, &output_backend, &opts);

    log_operation_summary(
        feature,
        total_files,
        copied,
        ignored,
        failed,
        flags,
        scan_stats,
    );
    Ok(finalize(
        report_sink,
        flags,
        scan_stats,
        copied,
        ignored,
        failed,
        errors,
    ))
}

#[derive(Clone, Copy)]
struct ReportFlags {
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
}

fn log_scan_summary(feature: &'static str, total_files: usize, stats: VisitStats) {
    debug!(
        feature,
        operation = "scan_sources",
        result = "ok",
        total_files,
        skipped_empty = stats.skipped_empty,
        skipped_unreadable = stats.skipped_unreadable,
        walker_errors = stats.walker_errors,
        "scanned source files"
    );
}

fn log_operation_summary(
    feature: &'static str,
    total: usize,
    copied: usize,
    ignored: usize,
    failed: usize,
    flags: ReportFlags,
    stats: VisitStats,
) {
    debug!(
        feature,
        operation = "summary",
        result = summary_result(failed),
        total,
        copied,
        ignored,
        failed,
        dry_run = flags.dry_run,
        remove = flags.remove,
        include_non_media = flags.include_non_media,
        skipped_empty = stats.skipped_empty,
        skipped_unreadable = stats.skipped_unreadable,
        walker_errors = stats.walker_errors,
        "operation summary"
    );
}

fn finalize(
    sink: Option<&dyn ReportSink>,
    flags: ReportFlags,
    stats: VisitStats,
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
) -> CopyReport {
    let report = make_report(
        flags.dry_run,
        flags.remove,
        flags.include_non_media,
        stats,
        copied,
        ignored,
        failed,
        errors,
    );
    emit_report(sink, &report);
    report
}

// 重叠保护：source ⊆ output（canonical 前缀含相等）时，dedup 会把每个源文件判为
// output 中已存在的副本，move 模式下 remove 即删除文件自身——必须 fail fast。
fn ensure_sources_outside_output(sources: &[Source], output_prefix: &str) -> common::Result<()> {
    for (loc, _) in sources {
        let src_prefix = canonical_prefix(loc);
        if under_prefix(&src_prefix, output_prefix) {
            return Err(common::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "source {src_prefix} is inside output {output_prefix}; \
                     move would treat sources as duplicates of themselves"
                ),
            )));
        }
    }
    Ok(())
}

// 扫源建索引 + 重叠剔除 + EXIF/P3 富集；拆出让 copy_with_sidecar 保持在 100 行内。
fn build_source_index(
    sources: &[Source],
    output_prefix: &str,
    sidecar: Option<CandidateProvider>,
    feature: &'static str,
) -> Index {
    let mut source = Index::new();
    for (loc, backend) in sources {
        source.visit_location(loc, backend);
    }
    // output ⊂ source（就地归档，如 copy /photos -o /photos/archive）：把已归档
    // 文件从 source 索引剔除，否则它们会被再次复制 / 在 move 模式下被误删。
    let excluded = source.remove_under_prefix(output_prefix);
    if excluded > 0 {
        debug!(
            feature,
            operation = "exclude_output_subtree",
            result = "ok",
            excluded,
            output = %output_prefix,
            "excluded already-archived files under output from source index"
        );
    }
    source.parse_exif(configured_chrono_offset());
    // P3 富集：adapters 层注入的 sidecar 发现（XMP / Takeout），entities 只消费
    // 转换好的 Candidate（依赖倒置，协议细节不进 usecases）。
    if let Some(provider) = sidecar {
        source.enrich_candidates(provider);
    }
    source
}

// F1 分桶并行：按 fast_hash 分桶（BTreeMap 保序）+ 桶内 full_path 字典序，让同
// hash 组内 winner 顺序在并行下仍可重现；桶间 par_iter 并行执行，桶内串行处理
// 让 do_copy 的 output_index.exists → add 序列在每桶内保持原有 dedup 语义。
fn run_copy_loop(
    source: &Index,
    output_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    opts: &CopyOpts<'_>,
) -> (usize, usize, usize, Vec<ReportError>) {
    let mut output_index = Index::new();
    output_index.visit_location(output_loc, output_backend);
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
                    opts,
                    output_loc,
                    output_backend,
                    feature,
                )
            })
            .reduce(CopyDelta::default, CopyDelta::merge)
    });

    (delta.copied, delta.ignored, delta.failed, delta.errors)
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
                local.errors.push(ReportError {
                    path: src.full_path.to_string(),
                    message: msg,
                });
            }
        }
    }
    local
}

/// `par_iter` map-reduce 汇总项：每 worker 局部累加避免全局 `Mutex` 串行化；
/// rayon tree-reduce 归并到根，`errors` 用 `Vec::extend` 拼接。
#[derive(Default)]
struct CopyDelta {
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
}

impl CopyDelta {
    fn merge(mut a: Self, b: Self) -> Self {
        a.copied += b.copied;
        a.ignored += b.ignored;
        a.failed += b.failed;
        a.errors.extend(b.errors);
        a
    }
}

// 构造 CopyReport 值对象；抽出避免参数列表过长。
// scanned = 入索引文件数（indexed）+ walker 触达但跳过的（empty/unreadable/walker_errors）。
#[expect(
    clippy::too_many_arguments,
    reason = "report 字段与 run_copy_loop 返回值一一对应；合并结构体反而在唯一调用点更绕"
)]
fn make_report(
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    scan_stats: VisitStats,
    copied: usize,
    ignored: usize,
    failed: usize,
    errors: Vec<ReportError>,
) -> CopyReport {
    // indexed = copied + ignored + failed（do_copy 三态都来自已入索引的文件）。
    let indexed = copied + ignored + failed;
    let skipped_total =
        scan_stats.skipped_empty + scan_stats.skipped_unreadable + scan_stats.walker_errors;
    let scanned = indexed + usize::try_from(skipped_total).unwrap_or(usize::MAX);
    CopyReport {
        scanned,
        copied,
        ignored,
        failed,
        skipped_empty: scan_stats.skipped_empty,
        skipped_unreadable: scan_stats.skipped_unreadable,
        walker_errors: scan_stats.walker_errors,
        dry_run,
        remove,
        include_non_media,
        errors,
    }
}

// 结构化日志 summary 的 result 维度值：失败计数为 0 即 "ok"，否则 "partial"。
pub(super) fn summary_result(failed: usize) -> &'static str {
    if failed == 0 { "ok" } else { "partial" }
}

// canonical_prefix 已上提到 entities::common（4 个 use case 共用）。

// 通过注入的 sink 输出报告；None 时跳过（用 case 不知道协议与持久化细节）。
fn emit_report(sink: Option<&dyn ReportSink>, report: &CopyReport) {
    if let Some(s) = sink {
        s.write(&Report::Copy(report));
    }
}
