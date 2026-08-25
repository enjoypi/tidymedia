//! move-text-shot 扫描阶段：walk source → 按 size/sniff 过滤 → OCR 判定。
//! 每个 entry 产一个 [`SourceDelta`]，rayon `par_iter` 并行后 tree-reduce。

use std::io;
use std::io::Read;
use std::sync::Arc;

use dashmap::DashSet;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::delta::{SourceDelta, record_failure, reduce_delta};
use super::target::move_one;
use crate::entities::backend::{Backend, Entry, EntryKind};
use crate::entities::common::{self};
use crate::entities::uri::Location;
use crate::usecases::ocr::TextDetector;

/// MIME sniff 头部字节数（与 `entities::exif::mime::MIME_SNIFF_BYTES` 同口径）；
/// 非 image 文件仅读此长度即 skip，避免完整读入大视频/压缩包白耗 IO+内存。
const MIME_SNIFF_BYTES: usize = 256;

#[expect(
    clippy::too_many_arguments,
    reason = "参数透传一比一；折结构体在唯一调用点先 build 也冗余"
)]
pub(super) fn process_source(
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
pub(super) fn process_entry(
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

/// 见 [`common::entry_under_prefix`]（已上提 entities 单点，cull 共用）。
pub(super) fn is_entry_under_output(entry_loc: &Location, output_prefix: &str) -> bool {
    common::entry_under_prefix(entry_loc, output_prefix)
}

/// 先读前 [`MIME_SNIFF_BYTES`] 字节做 `is_image` 判定：非 image 返 `Ok(None)`；
/// image 则 `read_to_end` 剩余，配合 `Vec::with_capacity(size_hint)` 消除 geometric
/// growth 的 realloc/memcpy 开销。远端 backend `open_read` 是全字节入堆的已知
/// 限制（trait 文档），本 helper 至少避免了「白读 500 MB 非 image」的最坏情形。
pub(super) fn sniff_and_read(
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
    drain_reader_to_option(&mut *reader, buf)
}

/// `Read::read_to_end` + `Ok(Some(buf))` 一体化包装：sniff 成功后
/// `read_to_end` 的 `?` Err arm 需构造「首 N 字节 OK 后续 Err」的分段 reader
/// （fake 未支持），分段错误路径由测试注入 reader error 覆盖。
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

pub(super) fn is_image(bytes: &[u8]) -> bool {
    infer::get(bytes).is_some_and(|t| t.mime_type().starts_with("image/"))
}
