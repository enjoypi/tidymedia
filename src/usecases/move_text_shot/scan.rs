//! move-text-shot 扫描阶段：walk source → 按 size/sniff 过滤 → OCR 判定。
//! 每个 entry 产一个 [`SourceDelta`]，rayon `par_iter` 并行后 tree-reduce。

use std::io;
use std::io::Read;
use std::sync::Arc;

use dashmap::DashSet;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::delta::{SourceDelta, record_failure, reduce_delta};
use super::target::move_one;
use crate::entities::backend::{Backend, Entry, EntryKind, FileBytes};
use crate::entities::common::{self};
use crate::entities::memory::Reservation;
use crate::entities::uri::Location;
use crate::usecases::ocr::TextDetector;

/// MIME sniff 头部字节数（与 `entities::exif::mime::MIME_SNIFF_BYTES` 同口径）；
/// 非 image 文件仅读此长度即 skip，避免完整读入大视频/压缩包白耗 IO+内存。
const MIME_SNIFF_BYTES: usize = 256;
const ENTRY_CHUNK: usize = 4096;

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
    let process = |chunk: Vec<Entry>| {
        chunk
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
            .reduce(SourceDelta::default, reduce_delta)
    };
    in_chunks(
        src_backend.walk(source),
        ENTRY_CHUNK,
        &source.display(),
        process,
    )
}

pub(super) fn in_chunks(
    walk: impl Iterator<Item = io::Result<Entry>>,
    chunk_len: usize,
    root: &str,
    process: impl Fn(Vec<Entry>) -> SourceDelta,
) -> SourceDelta {
    let mut total = SourceDelta::default();
    let mut chunk: Vec<Entry> = Vec::with_capacity(chunk_len);
    for res in walk {
        match res {
            Ok(e) => chunk.push(e),
            Err(err) => record_failure(&mut total, root.to_owned(), &err),
        }
        if chunk.len() == chunk_len {
            total = reduce_delta(total, process(std::mem::take(&mut chunk)));
        }
    }
    reduce_delta(total, process(chunk))
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
    let Some(_hold) = hold_bytes(entry.size, max_image_bytes) else {
        delta.skipped_too_large += 1;
        return delta;
    };
    let Some(bytes) = read_image(src_backend, &entry, &mut delta) else {
        return delta;
    };
    delta.image_files += 1;
    match detector.has_text(entry.location.path(), &bytes) {
        Ok(true) if !bytes.unchanged() => {
            let changed = io::Error::other("source changed while it was being read");
            record_failure(&mut delta, entry.location.display(), &changed);
        }
        Ok(true) => {
            delta.ocr_hits += 1;
            move_one(
                src_backend,
                &entry.location,
                source,
                output,
                output_backend,
                bytes,
                mkdir_cache,
                dry_run,
                &mut delta,
            );
        }
        Ok(false) => {
            delta.skipped_no_text += 1;
        }
        Err(e) if e.kind() == io::ErrorKind::FileTooLarge => {
            delta.skipped_too_large += 1;
        }
        Err(e) => {
            record_failure(&mut delta, entry.location.display(), &e);
        }
    }
    delta
}

fn hold_bytes(size: u64, max_image_bytes: u64) -> Option<Reservation<'static>> {
    let hold = &crate::usecases::memory::governor().hold;
    if size > max_image_bytes.min(hold.capacity()) {
        return None;
    }
    hold.reserve(size).ok()
}

fn read_image(
    src_backend: &Arc<dyn Backend>,
    entry: &Entry,
    delta: &mut SourceDelta,
) -> Option<FileBytes> {
    match sniff_and_read(src_backend, &entry.location, entry.size) {
        Ok(Some(bytes)) => Some(bytes),
        Ok(None) => {
            delta.skipped_non_image += 1;
            None
        }
        Err(e) => {
            record_failure(delta, entry.location.display(), &e);
            None
        }
    }
}

/// 见 [`common::entry_under_prefix`]（已上提 entities 单点）。
pub(super) fn is_entry_under_output(entry_loc: &Location, output_prefix: &str) -> bool {
    common::entry_under_prefix(entry_loc, output_prefix)
}

/// 先读前 [`MIME_SNIFF_BYTES`] 字节做 `is_image` 判定：非 image 返 `Ok(None)`；
/// image 则 `read_to_end` 剩余，配合 `Vec::with_capacity(size_hint)` 消除 geometric
pub(super) fn sniff_and_read(
    backend: &Arc<dyn Backend>,
    loc: &Location,
    size_hint: u64,
) -> io::Result<Option<FileBytes>> {
    if let Some(mapped) = backend.map_whole(loc)? {
        let bytes = FileBytes::Mapped(mapped);
        let head = &bytes[..bytes.len().min(MIME_SNIFF_BYTES)];
        return Ok(is_image(head).then_some(bytes));
    }
    let mut reader = backend.open_read(loc)?;
    // 预分配整文件大小；hint 与真实字节可能微差不影响正确性。
    // u64→usize 用 try_from 让 32-bit 平台大于 usize::MAX 时回退到未预分配（Vec::new()）。
    // 先填 sniff 窗口；若文件比 MIME_SNIFF_BYTES 小，read 会提前 EOF
    let mut head = [0u8; MIME_SNIFF_BYTES];
    let sniff_len = read_up_to(&mut *reader, &mut head)?;
    if !is_image(&head[..sniff_len]) {
        return Ok(None);
    }
    let mut buf = Vec::with_capacity(usize::try_from(size_hint).unwrap_or(0));
    buf.extend_from_slice(&head[..sniff_len]);
    drain_reader_to_option(&mut *reader, buf)
}

/// `Read::read_to_end` + `Ok(Some(buf))` 一体化包装：sniff 成功后
/// `read_to_end` 的 `?` Err arm 需构造「首 N 字节 OK 后续 Err」的分段 reader
/// （fake 未支持），分段错误路径由测试注入 reader error 覆盖。
fn drain_reader_to_option(
    reader: &mut dyn Read,
    mut buf: Vec<u8>,
) -> io::Result<Option<FileBytes>> {
    reader.read_to_end(&mut buf)?;
    Ok(Some(FileBytes::Heap(buf)))
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

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
