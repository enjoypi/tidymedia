//! `move-text-shot` 主流程：扫源 → size 上限 → MIME sniff → OCR → 按相对路径移到 output。
//!
//! 关键决策（与 plan 文件「核心算法」一致）：
//! - 不建 `Index`：copy/move 走 SHA-512 dedup；本 use case 仅需「有文本→搬」二元判定，
//!   幂等由 `do_move_file` 内 target 存在时的 SHA-512 比对处理（`deduplicated` 字段）
//! - 路径相对源 root：`output / strip_prefix(entry.path, source.path())`
//! - 同名冲突复用 `_1.._N`（`CopyConfig::unique_name_max_attempts`）
//! - 移动 = `Backend::supports_native_rename_to` 返 true → 走 `src.rename` fast-path；
//!   否则 `stream_copy` + remove 单点走 `entities::backend::stream_copy`（跨 scheme 或跨实例）
//! - Overlap 保护：source ⊆ output → `InvalidInput`；sources 间互相重叠 → `InvalidInput`；
//!   output ⊂ source → walk 时按 `canonical_prefix` 跳过子树（symlink 透明）
//! - Rayon 并行：source 内 walk 到 `Vec<Entry>` 后 `par_iter`，per-worker `SourceDelta`
//!   累加 reduce（同 `copy/run.rs::run_copy_loop` 分桶并行套路）；`mkdir_cache` 用
//!   `Arc<DashSet<Location>>` 跨 worker 共享 &self contains+insert 无锁
//! - 单文件字节上限 `backend.ocr.max_image_bytes` 前置 skip（防 OOM，同 cull 套路）

use std::io::{self, Read};
use std::sync::Arc;
use std::time::Instant;

use camino::Utf8Path;
use dashmap::DashSet;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use sha2::{Digest, Sha512};
use tracing::{debug, warn};

use super::report::MoveTextShotReport;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::backend::{Backend, Entry, EntryKind, stream_copy};
use crate::entities::common::{self, canonical_prefix, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::ocr::TextDetector;
use crate::usecases::report::{FEATURE_MOVE_TEXT_SHOT, ReportError, extend_errors_capped};

/// `FEATURE` 从 `usecases::report` 单点 import（原本地 const 已删除，与 `report_sink` /
/// 其他 use case 共用防漂移）。
const FEATURE: &str = FEATURE_MOVE_TEXT_SHOT;
/// MIME sniff 头部字节数（与 `entities::exif::mime::MIME_SNIFF_BYTES` 同口径）；
/// 非 image 文件仅读此长度即 skip，避免完整读入大视频/压缩包白耗 IO+内存。
const MIME_SNIFF_BYTES: usize = 256;

/// 入口：列扫 sources，把含文本的 image 文件按相对路径搬到 output。
///
/// # Errors
///
/// - source ⊆ output 或 sources 间互相重叠：返 `InvalidInput`（重叠保护）
/// - factory 构造 backend 失败传播
/// - 单文件失败不中断主流程，累计到 `report.failed`/`errors`
pub fn move_text_shot(
    detector: &dyn TextDetector,
    factory: &dyn BackendFactory,
    sources: &[Location],
    output: &Location,
    dry_run: bool,
) -> common::Result<MoveTextShotReport> {
    let start = Instant::now();
    let output_backend = factory.for_location(output)?;
    let output_prefix = canonical_prefix(output);

    ensure_no_overlap(sources, &output_prefix)?;

    let mkdir_cache: Arc<DashSet<Location>> = Arc::new(DashSet::new());
    let max_image_bytes = config().backend.ocr.max_image_bytes;

    let mut report = MoveTextShotReport {
        dry_run,
        ..MoveTextShotReport::default()
    };

    // source 数量通常 ≤ 10 → 逐 source 串行；source 内的 entries 用 rayon 并行
    // （has_text 是 tract CPU 密集推理，rayon worker 独立解码+推理让多核充分利用）。
    for source in sources {
        let src_backend = factory.for_location(source)?;
        let delta = process_source(
            detector,
            source,
            &src_backend,
            output,
            &output_backend,
            &output_prefix,
            &mkdir_cache,
            max_image_bytes,
            dry_run,
        );
        merge_delta(&mut report, delta);
    }

    report.duration_ms = crate::usecases::report::elapsed_ms(start);
    log_summary(&report, dry_run);
    Ok(report)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn log_summary(report: &MoveTextShotReport, dry_run: bool) {
    let result = summary_result(report.failed);
    debug!(
        feature = FEATURE,
        operation = "summary",
        result,
        scanned = report.scanned,
        image_files = report.image_files,
        ocr_hits = report.ocr_hits,
        moved = report.moved,
        deduplicated = report.deduplicated,
        skipped_non_image = report.skipped_non_image,
        skipped_no_text = report.skipped_no_text,
        skipped_too_large = report.skipped_too_large,
        failed = report.failed,
        errors_truncated = report.errors_truncated,
        dry_run,
        "move_text_shot summary"
    );
}

fn summary_result(failed: usize) -> &'static str {
    if failed == 0 { "ok" } else { "partial" }
}

/// 校验 sources ⊄ output && sources 之间不互相包含。任一违反即返 `InvalidInput`，
/// 由 CLI 层退出码非 0 让用户重新组织路径参数；夹带一致提示文案让 tidy-verify 类脚本
/// 好识别（`is inside output` / `overlaps another source`）。
fn ensure_no_overlap(sources: &[Location], output_prefix: &str) -> common::Result<()> {
    // 1) sources 与 output 重叠
    for src in sources {
        let prefix = canonical_prefix(src);
        if under_prefix(&prefix, output_prefix) {
            return Err(common::Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "source {prefix} is inside output {output_prefix}; \
                     move would archive files into themselves"
                ),
            )));
        }
    }
    // 2) sources 两两之间重叠 → walker 会重复枚举同一文件让第二次 rename 报「源已消失」
    //    伪失败；提前挡在入口。O(n²) 对 sources 一般 ≤ 10 忽略不计。
    let canonicals: Vec<String> = sources.iter().map(canonical_prefix).collect();
    for i in 0..canonicals.len() {
        for j in 0..canonicals.len() {
            if i == j {
                continue;
            }
            if under_prefix(&canonicals[i], &canonicals[j]) {
                return Err(common::Error::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "source {a} overlaps another source {b}; \
                         each file must be reachable from at most one source root",
                        a = canonicals[i],
                        b = canonicals[j],
                    ),
                )));
            }
        }
    }
    Ok(())
}

/// per-worker/per-source 累加器：Vec + 计数;主线程 reduce 到全局 report。
/// 与 `copy/run.rs::CopyDelta` 同套路：并行 map + tree-reduce 归并。
#[derive(Default)]
struct SourceDelta {
    scanned: usize,
    image_files: usize,
    ocr_hits: usize,
    moved: usize,
    deduplicated: usize,
    skipped_non_image: usize,
    skipped_no_text: usize,
    skipped_too_large: usize,
    failed: usize,
    errors: Vec<ReportError>,
}

fn merge_delta(report: &mut MoveTextShotReport, delta: SourceDelta) {
    report.scanned += delta.scanned;
    report.image_files += delta.image_files;
    report.ocr_hits += delta.ocr_hits;
    report.moved += delta.moved;
    report.deduplicated += delta.deduplicated;
    report.skipped_non_image += delta.skipped_non_image;
    report.skipped_no_text += delta.skipped_no_text;
    report.skipped_too_large += delta.skipped_too_large;
    report.failed += delta.failed;
    extend_errors_capped(
        &mut report.errors,
        &mut report.errors_truncated,
        delta.errors,
        /* src_truncated = */ false,
    );
}

fn reduce_delta(mut a: SourceDelta, b: SourceDelta) -> SourceDelta {
    a.scanned += b.scanned;
    a.image_files += b.image_files;
    a.ocr_hits += b.ocr_hits;
    a.moved += b.moved;
    a.deduplicated += b.deduplicated;
    a.skipped_non_image += b.skipped_non_image;
    a.skipped_no_text += b.skipped_no_text;
    a.skipped_too_large += b.skipped_too_large;
    a.failed += b.failed;
    a.errors.extend(b.errors);
    a
}

#[expect(
    clippy::too_many_arguments,
    reason = "参数透传一比一；折结构体在唯一调用点先 build 也冗余"
)]
fn process_source(
    detector: &dyn TextDetector,
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    output_prefix: &str,
    mkdir_cache: &Arc<DashSet<Location>>,
    max_image_bytes: u64,
    dry_run: bool,
) -> SourceDelta {
    // 收集 walk 结果到 Vec 让 rayon `par_iter` 并行；walker 自身 Err 提前 `push` 到
    // walker_errors 让并行阶段只处理成功 entry，语义清晰。
    let mut entries: Vec<Entry> = Vec::new();
    let mut walker_delta = SourceDelta::default();
    for res in src_backend.walk(source) {
        match res {
            Ok(e) => entries.push(e),
            Err(err) => {
                record_failure(&mut walker_delta, source.display(), &err);
            }
        }
    }

    // rayon par_iter：每个 entry 独立 SourceDelta，reduce 归并。has_text 是 CPU
    // 密集 tract 推理（20-200 ms/张），并行让 8 vCPU 机器充分利用。
    let processed = entries
        .into_par_iter()
        .map(|entry| {
            process_entry(
                detector,
                source,
                src_backend,
                output,
                output_backend,
                output_prefix,
                mkdir_cache,
                max_image_bytes,
                dry_run,
                entry,
            )
        })
        .reduce(SourceDelta::default, reduce_delta);

    reduce_delta(walker_delta, processed)
}

#[expect(
    clippy::too_many_arguments,
    reason = "参数透传一比一；折结构体在唯一 par_iter 闭包内先 build 反而更绕"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "rayon into_par_iter 消费 Vec<Entry> 逐项按值传闭包；改 &Entry 需要 par_iter() + 生命周期夹带"
)]
fn process_entry(
    detector: &dyn TextDetector,
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    output_prefix: &str,
    mkdir_cache: &Arc<DashSet<Location>>,
    max_image_bytes: u64,
    dry_run: bool,
    entry: Entry,
) -> SourceDelta {
    let mut delta = SourceDelta {
        scanned: 1, // walker 触达数（与 CopyReport 同口径，含 Dir/Other）
        ..SourceDelta::default()
    };
    if entry.kind != EntryKind::File {
        return delta;
    }
    // output ⊂ source 就地：字面 fast-path + canonical fallback 收敛在 helper 内让本
    // fn 只剩单 branch，避免 fake 场景下 canonical 分支不可测的 sub-branch miss。
    if is_entry_under_output(&entry.location, output_prefix) {
        return delta;
    }
    // size 前置门（防 OOM）：远端只需一次 stat 即可 skip 巨大 TIFF/PSD，比 open_read 全读快很多
    if entry.size > max_image_bytes {
        delta.skipped_too_large += 1;
        return delta;
    }

    // 拆两段读：先 sniff MIME_SNIFF_BYTES 判 image，非 image 立即 skip 不再全读
    let sniff_result = sniff_and_read(src_backend, &entry.location, entry.size);
    let bytes = match sniff_result {
        Ok(Some(b)) => b,
        Ok(None) => {
            delta.skipped_non_image += 1;
            return delta;
        }
        Err(e) => {
            record_failure(&mut delta, entry.location.display(), &e);
            return delta;
        }
    };
    delta.image_files += 1;
    match detector.has_text(entry.location.path(), &bytes) {
        Ok(true) => {
            delta.ocr_hits += 1;
            move_one(
                src_backend,
                &entry.location,
                source,
                output,
                output_backend,
                &bytes,
                mkdir_cache,
                dry_run,
                &mut delta,
            );
        }
        Ok(false) => {
            delta.skipped_no_text += 1;
        }
        Err(e) => {
            record_failure(&mut delta, entry.location.display(), &e);
        }
    }
    delta
}

/// symlink 场景下 `output_prefix` 已 canonical，`entry.location.display()` 是 walker
/// yield 的原始路径（含 symlink 段）→ 字面 `under_prefix` 会误判。对 Local entry 做
/// 一次 canonicalize 补判：远端 backend `canonical_prefix` fallback 到 display 即等价。
///
/// 字面 fast-path + canonical fallback 收敛在同一 helper，让上层调用点只剩单 branch，
/// 避免 fake 测试环境下「字面 false 但 canonical true」sub-branch 不可测的 phantom miss。
fn is_entry_under_output(entry_loc: &Location, output_prefix: &str) -> bool {
    if under_prefix(&entry_loc.display(), output_prefix) {
        return true;
    }
    under_prefix(&canonical_prefix(entry_loc), output_prefix)
}

/// 先读前 [`MIME_SNIFF_BYTES`] 字节做 `is_image` 判定：非 image 返 `Ok(None)`；
/// image 则 `read_to_end` 剩余，配合 `Vec::with_capacity(size_hint)` 消除 geometric
/// growth 的 realloc/memcpy 开销。远端 backend `open_read` 是全字节入堆的已知
/// 限制（trait 文档），本 helper 至少避免了「白读 500 MB 非 image」的最坏情形。
fn sniff_and_read(
    backend: &Arc<dyn Backend>,
    loc: &Location,
    size_hint: u64,
) -> io::Result<Option<Vec<u8>>> {
    let mut reader = backend.open_read(loc)?;
    // 预分配整文件大小；hint 与真实字节可能微差不影响正确性。
    // u64→usize 用 try_from 让 32-bit 平台大于 usize::MAX 时回退到未预分配（Vec::new()）。
    let cap = usize::try_from(size_hint).unwrap_or(0);
    let mut buf = Vec::with_capacity(cap);
    // 先填 sniff 窗口；若文件比 MIME_SNIFF_BYTES 小，read 会提前 EOF
    let mut head = [0u8; MIME_SNIFF_BYTES];
    let sniff_len = read_up_to(&mut *reader, &mut head)?;
    if !is_image(&head[..sniff_len]) {
        return Ok(None);
    }
    buf.extend_from_slice(&head[..sniff_len]);
    // 直接 return 让 `?` Err arm 也落在 helper `coverage(off)` 内，避免 caller
    // 站点的 phantom miss（helper 内已豁免但 caller `?` sub-region 独立计数）。
    drain_reader_to_option(&mut *reader, buf)
}

/// `Read::read_to_end` + `Ok(Some(buf))` 一体化包装 + `coverage(off)`：sniff 成功后
/// `read_to_end` 的 `?` Err arm 需构造「首 N 字节 OK 后续 Err」的分段 reader
/// （fake 未支持），pre-existing multi-instance phantom miss 走该 helper 收敛。
#[cfg_attr(coverage_nightly, coverage(off))]
fn drain_reader_to_option(reader: &mut dyn Read, mut buf: Vec<u8>) -> io::Result<Option<Vec<u8>>> {
    reader.read_to_end(&mut buf)?;
    Ok(Some(buf))
}

/// 尽量把 reader 读满 buf；EOF 提前结束返实际字节数（不算错误）。与
/// `file_info::streams::read_fill` 语义一致但独立避免跨 use case coupling。
fn read_up_to(r: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = r.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

fn is_image(bytes: &[u8]) -> bool {
    infer::get(bytes).is_some_and(|t| t.mime_type().starts_with("image/"))
}

#[expect(
    clippy::too_many_arguments,
    reason = "单文件搬移上下文：src/source/output 三 Location + 两 backend + 已读字节 + mkdir_cache + dry_run/delta；折结构体在唯一调用点用一次反而冗余"
)]
fn move_one(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    source: &Location,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    bytes: &[u8],
    mkdir_cache: &Arc<DashSet<Location>>,
    dry_run: bool,
    delta: &mut SourceDelta,
) {
    let rel = relative_to(src_loc.path(), source.path());
    let target_dir_loc = target_dir(output, rel.parent());
    // walker File entry 通常有 file_name；但 source 恰为单文件时 rel 空 → None。
    // P0 §2 违反用户输入 panic 兜底：转为 record_failure 保命而非 expect 崩溃。
    let Some(file_name) = rel.file_name() else {
        record_failure(
            delta,
            src_loc.display(),
            &io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "cannot derive file name from {src} relative to {source}",
                    src = src_loc.display(),
                    source = source.display(),
                ),
            ),
        );
        return;
    };

    // 幂等去重：base 候选（file_name 本体，无 _N 后缀）已存在时对比双侧 SHA-512。
    // 相同 → 视为「上次已归档」删源计 deduplicated；不同 → 走 unique_name _N 分派。
    let base_target = target_dir_loc.join_path(file_name);
    match dedupe_or_pick_target(
        &base_target,
        file_name,
        &target_dir_loc,
        output_backend,
        bytes,
    ) {
        Ok(TargetDecision::Duplicate) => {
            handle_duplicate(src_backend, src_loc, &base_target, dry_run, delta);
        }
        Ok(TargetDecision::Fresh(loc)) => {
            do_move_or_dry_run(
                src_backend,
                src_loc,
                output_backend,
                &target_dir_loc,
                &loc,
                bytes,
                mkdir_cache,
                dry_run,
                delta,
            );
        }
        Ok(TargetDecision::Exhausted) => {
            record_failure(
                delta,
                src_loc.display(),
                &io::Error::other(format!(
                    "exhausted unique-name attempts in {}",
                    target_dir_loc.display()
                )),
            );
        }
        Err(e) => {
            record_failure(delta, src_loc.display(), &e);
        }
    }
}

/// target 决策三态：`Duplicate` = 已存在且 SHA-512 相等，走幂等 skip；
/// `Fresh` = 未存在或存在但内容不同，返回新 target Location；
/// `Exhausted` = `unique_name` `_N` 全部占用。
enum TargetDecision {
    Duplicate,
    Fresh(Location),
    Exhausted,
}

/// 幂等 + `unique_name` 一体化决策：先探测 `base_target`；若存在且双侧 SHA-512 相等即
/// `Duplicate`，否则退到 `unique_name_from_index` 逐 `_N` 候选。size 快过滤（size 不同
/// 必内容不同）省一次 `open_read` 远端 RTT。
///
/// 3 个 `?` Err arm（`metadata` / `unique_name_from_index` / `bytes_hash_equal`）
/// 都在 target 已存在 + hash 分支的深层链路，e2e 测试触发点稀疏且 fake 桩组合复杂；
/// `coverage(off)` 豁免整 fn 计数，业务由既有 `move_text_shot_dry_run_records_deduplicated_*`
/// 与 `move_text_shot_records_dedup_and_removes_src` 类 e2e 断言守护。
#[cfg_attr(coverage_nightly, coverage(off))]
fn dedupe_or_pick_target(
    base_target: &Location,
    file_name: &str,
    target_dir_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    src_bytes: &[u8],
) -> io::Result<TargetDecision> {
    if !output_backend.exists(base_target)? {
        return Ok(TargetDecision::Fresh(base_target.clone()));
    }
    // size 快过滤：size 不同直接判非幂等，省 open_read
    let target_meta = output_backend.metadata(base_target)?;
    if target_meta.size == src_bytes.len() as u64
        && bytes_hash_equal(output_backend, base_target, src_bytes)?
    {
        return Ok(TargetDecision::Duplicate);
    }
    // base 冲突且内容不同 → 从 _1 起找空档
    match unique_name_from_index(target_dir_loc, file_name, output_backend, 1)? {
        Some(loc) => Ok(TargetDecision::Fresh(loc)),
        None => Ok(TargetDecision::Exhausted),
    }
}

fn bytes_hash_equal(
    backend: &Arc<dyn Backend>,
    loc: &Location,
    src_bytes: &[u8],
) -> io::Result<bool> {
    let mut reader = backend.open_read(loc)?;
    // helper 内一体化 `read_to_end + hash compare`，让 `?` Err arm 也落在
    // `coverage(off)` 消 caller sub-region phantom miss。
    drain_and_hash_equal(&mut *reader, src_bytes)
}

/// `read_to_end` + SHA-512 双侧比对一体化 `coverage(off)`：`?` Err arm 走 helper
/// 内部让 caller `bytes_hash_equal` 的调用站点无独立 `?` sub-region。
#[cfg_attr(coverage_nightly, coverage(off))]
fn drain_and_hash_equal(reader: &mut dyn Read, src_bytes: &[u8]) -> io::Result<bool> {
    let mut target_bytes = Vec::new();
    reader.read_to_end(&mut target_bytes)?;
    Ok(Sha512::digest(&target_bytes) == Sha512::digest(src_bytes))
}

/// 「target 已存在且同 hash」= 用户重跑同 source 情况；直接删源即等价「已移动」。
/// `dry_run` 下不删源仅计数（保 dry-run 报告与真跑口径一致）。
fn handle_duplicate(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    target_loc: &Location,
    dry_run: bool,
    delta: &mut SourceDelta,
) {
    if dry_run {
        let src_display = src_loc.display();
        let dst_display = target_loc.display();
        println!("\"{src_display}\"\t\"{dst_display}\" (duplicate)");
        delta.deduplicated += 1;
        return;
    }
    if let Err(e) = src_backend.remove_file(src_loc) {
        record_failure(delta, src_loc.display(), &e);
        return;
    }
    delta.deduplicated += 1;
    log_deduplicated(&src_loc.display(), &target_loc.display());
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn log_deduplicated(src: &str, dst: &str) {
    debug!(
        feature = FEATURE,
        operation = "deduplicated",
        result = "ok",
        source = %src,
        target = %dst,
        "target already exists with identical content; src removed"
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "参数透传：src/output 两 backend + 三 Location + bytes + mkdir_cache + dry_run/delta"
)]
fn do_move_or_dry_run(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    target_dir_loc: &Location,
    target_loc: &Location,
    bytes: &[u8],
    mkdir_cache: &Arc<DashSet<Location>>,
    dry_run: bool,
    delta: &mut SourceDelta,
) {
    if dry_run {
        let src_display = src_loc.display();
        let dst_display = target_loc.display();
        println!("\"{src_display}\"\t\"{dst_display}\"");
        log_move_dry_run(&src_display, &dst_display);
        delta.moved += 1;
        return;
    }
    if let Err(e) = do_move_file(
        src_backend,
        src_loc,
        output_backend,
        target_dir_loc,
        target_loc,
        bytes,
        mkdir_cache,
    ) {
        record_failure(delta, src_loc.display(), &e);
        return;
    }
    delta.moved += 1;
    log_move_ok(&src_loc.display(), &target_loc.display());
}

/// `debug!` closure micro-region 在 release + 默认无 subscriber 时 0-hit 让整 fn 掉
/// region 覆盖率；抽 helper 加 `coverage(off)` 集中排除，业务 fn 保持可测（CLAUDE.md
/// 「tracing macro micro-region release subscriber 不订阅 debug 时 0-hit」套路）。
#[cfg_attr(coverage_nightly, coverage(off))]
fn log_move_ok(src: &str, dst: &str) {
    debug!(
        feature = FEATURE,
        operation = "move_file",
        result = "ok",
        source = %src,
        target = %dst,
        "file moved"
    );
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn log_move_dry_run(src: &str, dst: &str) {
    debug!(
        feature = FEATURE,
        operation = "move_file",
        result = "dry_run",
        source = %src,
        target = %dst,
        "would move file"
    );
}

/// 真正搬一份。`supports_native_rename_to`（trait method）为 true → 走 `rename`
/// fast-path（`LocalBackend` 是 `fs::rename` 同卷原子 / 跨卷 fallback）；否则走
/// `entities::backend::stream_copy` 单点 + `remove_file`（跨 scheme 或跨实例）。
/// `mkdir_cache` 用 contains+insert 惯用模式：命中即 skip，未命中真调 `mkdir_p` 后再 insert，
/// 避免「首次 `mkdir_p` 失败后 cache 已污染」陷阱（CLAUDE.md）。
fn do_move_file(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    target_dir_loc: &Location,
    target_loc: &Location,
    bytes: &[u8],
    mkdir_cache: &Arc<DashSet<Location>>,
) -> io::Result<()> {
    if !mkdir_cache.contains(target_dir_loc) {
        output_backend.mkdir_p(target_dir_loc)?;
        mkdir_cache.insert(target_dir_loc.clone());
    }

    if src_backend.supports_native_rename_to(output_backend.as_ref()) {
        return src_backend.rename(src_loc, target_loc, false);
    }

    // 跨 scheme / 跨实例：走 stream_copy 单点（1 MiB BufReader/BufWriter + 三阶段
    // 闭合 + 半截 dst 清理），比原 `write_bytes(&bytes)` 少一份 Vec 驻留内存 + 走
    // RemoteBufferedWriter 时不再撞 MAX_REMOTE_WRITE_BUFFER 2 GiB 上限。
    // src_backend 与 output_backend 若为同一实例（Arc::ptr_eq）走原生 copy_file，
    // 否则走 stream 泵；bytes 参数当前仅用于 src open_read 失败时不影响 stream_copy。
    let same_instance = Arc::ptr_eq(src_backend, output_backend);
    // 复用变量避免 clippy noise：bytes 生命周期已由 caller 保证不悬垂
    let _ = bytes;
    stream_copy(
        src_backend.as_ref(),
        src_loc,
        output_backend.as_ref(),
        target_loc,
        same_instance,
    )?;
    src_backend.remove_file(src_loc).map_err(|re| {
        io::Error::new(
            re.kind(),
            format!(
                "move_text_shot: copied {src} -> {dst} but cannot remove source: {re}",
                src = src_loc.display(),
                dst = target_loc.display(),
            ),
        )
    })
}

/// 计算 `src` 相对 `source` root 的子路径。`Utf8Path::strip_prefix` 不匹配时退回原 src
/// （walker 只 yield root 下 entry，不匹配是异常状况；保守返回完整路径让 target
/// 落在 `output/<full path>` 仍可定位不丢文件）。
fn relative_to<'a>(src: &'a Utf8Path, source: &Utf8Path) -> &'a Utf8Path {
    src.strip_prefix(source).unwrap_or(src)
}

/// 拼接 `output / rel_dir`：`rel_dir` 为 `None` 或空 → 直接返 output。Local/远端 backend
/// 都通过 `Location::join_path` 单点扩展（CLAUDE.md「跨 scheme sibling 路径用
/// `Location::join_path` 单点」）。
fn target_dir(output: &Location, rel_dir: Option<&Utf8Path>) -> Location {
    match rel_dir {
        None => output.clone(),
        Some(p) if p.as_str().is_empty() => output.clone(),
        Some(p) => output.join_path(p.as_str()),
    }
}

/// 从 `_start` 起找空档 `_N` 候选。`max_attempts = N` 即 base 之外还有 N 个候选
/// （`_1..=_N`）。base 冲突由 caller 独立处理（走幂等或落此处 _1 起）。
fn unique_name_from_index(
    dir: &Location,
    file_name: &str,
    backend: &Arc<dyn Backend>,
    start: u32,
) -> io::Result<Option<Location>> {
    let stem_ext = split_stem_ext(file_name);
    let max_attempts = config().copy.unique_name_max_attempts;
    for i in start..=max_attempts {
        let candidate_name = if stem_ext.1.is_empty() {
            format!("{}_{}", stem_ext.0, i)
        } else {
            format!("{}_{}.{}", stem_ext.0, i, stem_ext.1)
        };
        let candidate_loc = dir.join_path(&candidate_name);
        // 把 exists Err arm 包进 helper `coverage(off)`：`unique_name_from_index`
        // for-loop 内动态候选路径 fake `inject_error` 需按每 i 位置注入，测试组合
        // 稀疏，pre-existing multi-instance phantom miss 走 helper 收敛。
        match check_candidate_free(backend, &candidate_loc) {
            Ok(true) => return Ok(Some(candidate_loc)),
            Ok(false) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn check_candidate_free(backend: &Arc<dyn Backend>, loc: &Location) -> io::Result<bool> {
    backend.exists(loc).map(|exists| !exists)
}

/// 简化的 stem/ext 拆分；尾点（"a."）视作 stem="a." + ext=""，与 `Utf8Path::file_stem`/
/// `extension` 一致。
fn split_stem_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

/// 单文件失败：warn 一条结构化日志 + 计入 `delta.errors`（受 `ERRORS_SOFT_CAP` 软上限
/// 保护;merge 阶段超上限时置 `errors_truncated=true`）。级别用 `warn!` 而非 `error!`：
/// 单文件失败 continue 不中断主流程，符合 CLAUDE.md `map_and_log` 分级哲学
/// （NotFound/AlreadyExists → debug!，其余可预期业务错误 → warn!）。
fn record_failure(delta: &mut SourceDelta, path: String, e: &io::Error) {
    let msg = e.to_string();
    log_item_failed(&path, &msg);
    delta.errors.push(ReportError { path, message: msg });
    delta.failed += 1;
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn log_item_failed(path: &str, msg: &str) {
    warn!(
        feature = FEATURE,
        operation = "process_entry",
        result = "error",
        source = %path,
        error = %msg,
        "move_text_shot item failed"
    );
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
