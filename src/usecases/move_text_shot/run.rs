//! `move-text-shot` 主流程：扫源 → MIME 过滤 → OCR → 按相对路径移到 output。
//!
//! 关键决策（与 plan 文件「核心算法」一致）：
//! - 不建 `Index`：去重不需要；建索引会读 4 KiB fast hash 是纯开销
//! - 路径相对源 root：`output / strip_prefix(entry.path, source.path())`
//! - 同名冲突复用 `_1.._N`（`CopyConfig::unique_name_max_attempts`）
//! - 移动 = 同 local + remove → `rename` fast-path；跨 backend → `stream_copy + remove`
//! - Overlap 保护：source ⊆ output → InvalidInput；output ⊂ source → walk 时跳过子树

use std::io::{self, Read};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use tracing::{debug, error};

use super::report::MoveTextShotReport;
use crate::adapters::backend::factory::BackendFactory;
use crate::adapters::ocr::TextDetector;
use crate::entities::backend::{Backend, EntryKind};
use crate::entities::common::{self, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::report::ReportError;

const FEATURE: &str = "move_text_shot";
/// infer 实际只看前 16-32 字节，与 `entities::exif::mime::MIME_SNIFF_BYTES` 同口径。
const MIME_SNIFF_BYTES: usize = 256;

/// 入口：列扫 sources，把含文本的 image 文件按相对路径搬到 output。
///
/// # Errors
///
/// - source ⊆ output（重叠保护）返 `InvalidInput`
/// - factory 构造 backend 失败、output `mkdir_p` 失败传播
/// - 单文件失败不中断主流程，累计到 `report.failed`/`errors`
pub fn move_text_shot(
    detector: &dyn TextDetector,
    factory: &dyn BackendFactory,
    sources: &[Location],
    output: &Location,
    dry_run: bool,
) -> common::Result<MoveTextShotReport> {
    let output_backend = factory.for_location(output)?;
    let output_prefix = output.display();

    ensure_sources_outside_output(sources, &output_prefix)?;

    let mut report = MoveTextShotReport {
        dry_run,
        ..MoveTextShotReport::default()
    };

    if !dry_run {
        output_backend.mkdir_p(output)?;
    }

    for source in sources {
        let src_backend = factory.for_location(source)?;
        process_source(
            detector,
            source,
            &src_backend,
            output,
            &output_backend,
            &output_prefix,
            dry_run,
            &mut report,
        );
    }

    let result = summary_result(report.failed);
    debug!(
        feature = FEATURE,
        operation = "summary",
        result,
        scanned = report.scanned,
        image_files = report.image_files,
        ocr_hits = report.ocr_hits,
        moved = report.moved,
        skipped_non_image = report.skipped_non_image,
        skipped_no_text = report.skipped_no_text,
        failed = report.failed,
        dry_run,
        "move_text_shot summary"
    );

    Ok(report)
}

fn summary_result(failed: usize) -> &'static str {
    if failed == 0 { "ok" } else { "partial" }
}

fn ensure_sources_outside_output(sources: &[Location], output_prefix: &str) -> common::Result<()> {
    for src in sources {
        let prefix = src.display();
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
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "一比一参数透传；折结构体后调用点仍要先 build 该结构体"
)]
fn process_source(
    detector: &dyn TextDetector,
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    output_prefix: &str,
    dry_run: bool,
    report: &mut MoveTextShotReport,
) {
    for entry in src_backend.walk(source) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                record_failure(report, source.display(), &e);
                continue;
            }
        };
        if entry.kind != EntryKind::File {
            continue;
        }
        // output ⊂ source 就地：跳过已在 output 下的 entry，避免再次搬移已归档文件
        if under_prefix(&entry.location.display(), output_prefix) {
            continue;
        }
        report.scanned += 1;
        process_entry(
            detector,
            source,
            src_backend,
            &entry.location,
            output,
            output_backend,
            dry_run,
            report,
        );
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "一比一参数透传；本函数只做单文件主路径调度"
)]
fn process_entry(
    detector: &dyn TextDetector,
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    dry_run: bool,
    report: &mut MoveTextShotReport,
) {
    let bytes = match read_all(src_backend, src_loc) {
        Ok(b) => b,
        Err(e) => {
            record_failure(report, src_loc.display(), &e);
            return;
        }
    };
    if !is_image(&bytes) {
        report.skipped_non_image += 1;
        return;
    }
    report.image_files += 1;
    match detector.has_text(src_loc.path(), &bytes) {
        Ok(true) => {
            report.ocr_hits += 1;
            move_one(
                src_backend,
                src_loc,
                source,
                output,
                output_backend,
                &bytes,
                dry_run,
                report,
            );
        }
        Ok(false) => {
            report.skipped_no_text += 1;
        }
        Err(e) => {
            record_failure(report, src_loc.display(), &e);
        }
    }
}

/// 把 `src_backend` 上的 `src_loc` 全部字节读入堆。远端 backend `RemoteClient::read`
/// 整文件入堆是项目已知限制；本子命令需要把字节同时喂给 MIME 嗅探 + OCR，提前读完一份即可。
fn read_all(backend: &Arc<dyn Backend>, loc: &Location) -> io::Result<Vec<u8>> {
    let mut reader = backend.open_read(loc)?;
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    Ok(buf)
}

fn is_image(bytes: &[u8]) -> bool {
    let head_len = MIME_SNIFF_BYTES.min(bytes.len());
    infer::get(&bytes[..head_len]).is_some_and(|t| t.mime_type().starts_with("image/"))
}

#[expect(
    clippy::too_many_arguments,
    reason = "单文件搬移上下文：src/source/output 三 Location + 两 backend + 已读字节 + dry_run/report；折结构体仅在唯一调用点用一次反而冗余"
)]
fn move_one(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    source: &Location,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    bytes: &[u8],
    dry_run: bool,
    report: &mut MoveTextShotReport,
) {
    let rel = relative_to(src_loc.path(), source.path());
    let target_dir_loc = target_dir(output, rel.parent());
    // walker 只 yield File 类型 entry，路径必有文件名；expect 标内部不变量
    let file_name = rel
        .file_name()
        .expect("internal: walker file entry must have a file name");
    let target_loc = match unique_name_in_dir(&target_dir_loc, file_name, output_backend) {
        Ok(Some(loc)) => loc,
        Ok(None) => {
            record_failure(
                report,
                src_loc.display(),
                &io::Error::other(format!(
                    "exhausted unique-name attempts in {}",
                    target_dir_loc.display()
                )),
            );
            return;
        }
        Err(e) => {
            record_failure(report, src_loc.display(), &e);
            return;
        }
    };

    if dry_run {
        let src_display = src_loc.display();
        let dst_display = target_loc.display();
        // 与 copy/ops.rs 同套路：CLI 脚本可读输出走 stdout，业务日志走 tracing/stderr
        println!("\"{src_display}\"\t\"{dst_display}\"");
        debug!(
            feature = FEATURE,
            operation = "move_file",
            result = "dry_run",
            source = %src_display,
            target = %dst_display,
            "would move file"
        );
        report.moved += 1;
        return;
    }

    if let Err(e) = do_move_file(
        src_backend,
        src_loc,
        output_backend,
        &target_dir_loc,
        &target_loc,
        bytes,
    ) {
        record_failure(report, src_loc.display(), &e);
        return;
    }
    report.moved += 1;
    let src_display = src_loc.display();
    let dst_display = target_loc.display();
    debug!(
        feature = FEATURE,
        operation = "move_file",
        result = "ok",
        source = %src_display,
        target = %dst_display,
        "file moved"
    );
}

/// 真正搬一份。同 local + 同卷 → `LocalBackend::rename` 走 `fs::rename` fast-path；
/// 跨设备由 `Backend::rename` 默认 fallback 到 copy+remove；跨 scheme 没法 rename
/// → 用已读 `bytes` 直接 `output_backend.open_write` 写出 + src 删除，避免二次
/// `open_read`（`read_all` 已读过一次，二次读在并发删源场景仍可能失败但不在本路径
/// 服务的语义内）。
fn do_move_file(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    target_dir_loc: &Location,
    target_loc: &Location,
    bytes: &[u8],
) -> io::Result<()> {
    output_backend.mkdir_p(target_dir_loc)?;

    if src_backend.scheme() == output_backend.scheme() && src_backend.scheme() == "local" {
        // 同 LocalBackend：fs::rename（同卷原子 / 跨卷自动 fallback）。mkparents=false
        // 因为 mkdir_p 已建好父目录。
        return src_backend.rename(src_loc, target_loc, false);
    }

    // 跨 scheme：写已读字节 + remove。**非原子**——write 成功 remove 失败的半态
    // 错误必须文案标 "copied … but cannot remove source"，与 copy/ops.rs::remove_src_after_stream_copy
    // 同套路，让用户重跑时不误判。
    write_bytes(output_backend, target_loc, bytes)?;
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

fn write_bytes(
    output_backend: &Arc<dyn Backend>,
    target_loc: &Location,
    bytes: &[u8],
) -> io::Result<()> {
    let mut writer = output_backend.open_write(target_loc, false)?;
    let result = writer.write_all(bytes).and_then(|()| writer.finish());
    if let Err(e) = result {
        // open_write 已 create/truncate；中途失败必清半截，否则残留占位且无告警
        let _ = output_backend.remove_file(target_loc);
        return Err(e);
    }
    Ok(())
}

/// 计算 `src` 相对 `source` root 的子路径。`Utf8Path::strip_prefix` 不匹配时退回原 src
/// （理论上 walker 只 yield root 下 entry，不匹配是异常状况；保守返回完整路径让 target
/// 落在 `output/<full path>` 仍可定位，不丢文件）。
fn relative_to<'a>(src: &'a Utf8Path, source: &Utf8Path) -> &'a Utf8Path {
    src.strip_prefix(source).unwrap_or(src)
}

/// 拼接 `output / rel_dir`：`rel_dir` 为 `None` 或空 → 直接返 output。Local/远端 backend
/// 都通过 `Location::with_path` 单点扩展（CLAUDE.md「跨 scheme sibling 路径用 `with_path`
/// 单点」）。
fn target_dir(output: &Location, rel_dir: Option<&Utf8Path>) -> Location {
    let new_path = match rel_dir {
        None => output.path().to_path_buf(),
        Some(p) if p.as_str().is_empty() => output.path().to_path_buf(),
        Some(p) => output.path().join(p),
    };
    output.with_path(new_path)
}

/// 同 `copy::naming::generate_unique_name` 的 `_1.._N` 算法，但 template 固定为
/// 「dir / file」（无 `archive_template` 渲染）。`max_attempts = N` 即 N+1 候选（原名 + _1..=_N）。
fn unique_name_in_dir(
    dir: &Location,
    file_name: &str,
    backend: &Arc<dyn Backend>,
) -> io::Result<Option<Location>> {
    let stem_ext = split_stem_ext(file_name);
    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..=max_attempts {
        let candidate_name = if i == 0 {
            file_name.to_string()
        } else if stem_ext.1.is_empty() {
            format!("{}_{}", stem_ext.0, i)
        } else {
            format!("{}_{}.{}", stem_ext.0, i, stem_ext.1)
        };
        let candidate_path: Utf8PathBuf = dir.path().join(&candidate_name);
        let candidate_loc = dir.with_path(candidate_path);
        if !backend.exists(&candidate_loc)? {
            return Ok(Some(candidate_loc));
        }
    }
    Ok(None)
}

/// 简化的 stem/ext 拆分；尾点（"a."）视作 stem="a." + ext=""，与 `Utf8Path::file_stem`/
/// `extension` 一致。
fn split_stem_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

fn record_failure(report: &mut MoveTextShotReport, path: String, e: &io::Error) {
    let msg = e.to_string();
    error!(
        feature = FEATURE,
        operation = "process_entry",
        result = "error",
        source = %path,
        error = %msg,
        "move_text_shot item failed"
    );
    report.errors.push(ReportError { path, message: msg });
    report.failed += 1;
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
