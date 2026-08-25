//! cull 扫源阶段：walk → 过滤超大/非媒体 → 逐张 read+decode → pHash + 清晰度。
//! 产出 `ScannedFile`（仅 metadata + hash + sharpness），字节与解码图在 `scan_entry`
//! 内算完即 drop，避免整批图驻留 OOM。

use std::io;
use std::sync::Arc;

use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::phash::phash;
use super::report::CullReport;
use super::sharpness::laplacian_variance;
use super::util::{
    is_image, log_scan_entry_ok, log_scan_source_complete, log_scan_source_start, read_all,
    record_failure,
};
use crate::entities::backend::{Backend, Entry, EntryKind};
use crate::entities::common;
use crate::entities::uri::Location;
use crate::usecases::config::FaceConfig;

/// 单文件扫描结果。仅持 metadata + phash + sharpness：`raw_bytes` 与 `decoded` 在
/// `scan_entry` 内算完即 drop，`analyze_image` 阶段按需重读重 decode（避免整批图驻留 OOM）。
pub(super) struct ScannedFile {
    pub(super) src_loc: Location,
    pub(super) src_backend: Arc<dyn Backend>,
    pub(super) source_root: Location,
    pub(super) hash: u64,
    pub(super) sharpness: f32,
}

/// 单文件并行处理结果：`None` = 非图静默跳；`Err` = 该计入 failed 的 IO/decode 错误。
type ScanOutcome = Option<Result<ScannedFile, (String, io::Error)>>;

pub(super) fn scan_source(
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    output_prefix: &str,
    face_cfg: &FaceConfig,
    out: &mut Vec<ScannedFile>,
    report: &mut CullReport,
) {
    log_scan_source_start(&source.display(), face_cfg.max_image_bytes);
    // 阶段 1：walker 串行收集合法 file entries（walker 本身轻量，IO 重头在单文件
    // read+decode）；同时把 walker_errors 与超大跳过即时记 failed。
    let mut entries: Vec<Entry> = Vec::new();
    for entry in src_backend.walk(source) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                // walker_errors 也算 scanned：与 CopyReport.scanned 同口径
                //（= indexed + skipped_empty + skipped_unreadable + walker_errors）。
                // 旧实现只增 failed 不增 scanned 让汇总日志 failed > scanned 误导排查。
                report.scanned += 1;
                record_failure(report, source.display(), &e);
                continue;
            }
        };
        if entry.kind != EntryKind::File {
            continue;
        }
        // 命中 = 该文件位于 output 子树（同根归档场景），不算 source 触达。
        // entry_under_prefix 做字面 + canonical 双判：macOS `/var → /private/var`
        // 等 symlink 下 walker 字面路径与 canonical output prefix 不可比。
        if common::entry_under_prefix(&entry.location, output_prefix) {
            continue;
        }
        // 触达 source 文件即计入 scanned（含后续被识别为非图/超大/解码失败/IO 失败的）；
        // 口径与 CopyReport.scanned 一致：walker 触达数而非成功入索引数。
        report.scanned += 1;
        if entry.size > face_cfg.max_image_bytes {
            let err = io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "cull: file {bytes} bytes exceeds backend.face.max_image_bytes={limit}",
                    bytes = entry.size,
                    limit = face_cfg.max_image_bytes,
                ),
            );
            record_failure(report, entry.location.display(), &err);
            continue;
        }
        entries.push(entry);
    }

    // 阶段 2：并行 read+decode+phash+sharpness。Backend: Send+Sync + scan_entry 是纯函数
    // 让 rayon worker 安全调用；OOM 已修（不再缓存图），N 核同时驻留 N 张图安全。
    let results: Vec<ScanOutcome> = entries
        .into_par_iter()
        .map(|e| scan_entry(src_backend, &e.location, source))
        .collect();

    // 阶段 3：主线程合并结果（report 是 &mut，并发改造仅限 IO/decode 阶段）。
    let total_entries = results.len();
    let mut valid = 0_usize;
    for r in results {
        match r {
            None => {}
            Some(Ok(file)) => {
                valid += 1;
                out.push(file);
            }
            Some(Err((path, err))) => record_failure(report, path, &err),
        }
    }
    log_scan_source_complete(&source.display(), valid, total_entries);
}

/// 单文件 scan：读字节 → MIME 嗅探 → decode → pHash + 灰度清晰度。
/// 纯函数（不持 `&mut report`）让 rayon worker 并发安全；错误以 `(path, err)` 形式
/// 上抛由主线程统一 `record_failure`。
fn scan_entry(
    src_backend: &Arc<dyn Backend>,
    location: &Location,
    source: &Location,
) -> ScanOutcome {
    let bytes = match read_all(src_backend, location) {
        Ok(b) => b,
        Err(e) => return Some(Err((location.display(), e))),
    };
    if !is_image(&bytes) {
        return None;
    }
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            return Some(Err((
                location.display(),
                io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")),
            )));
        }
    };
    let hash = phash(&img);
    // grayscale 接 GenericImageView，直接传 &img 避免 DynamicImage::ImageRgb8(img.clone())
    // 整图克隆（20 MiB 大图 peak RSS 三倍放大致批量扫 OOM 风险）。
    let luma = image::imageops::grayscale(&img);
    let sharp = laplacian_variance(&luma);
    // P0 §14 业务 debug：单图特征供 AI 分析分组前分布。
    log_scan_entry_ok(&location.display(), bytes.len() as u64, hash, sharp);
    // scan 阶段产出仅含 metadata + hash + sharpness：bytes 与 img 走出作用域立即释放，
    // 避免整批图驻留 OOM；analyze_image 对组内成员重读+重 decode（多图组承担）。
    Some(Ok(ScannedFile {
        src_loc: location.clone(),
        src_backend: src_backend.clone(),
        source_root: source.clone(),
        hash,
        sharpness: sharp,
    }))
}
