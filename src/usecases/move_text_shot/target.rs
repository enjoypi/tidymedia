//! move-text-shot 单文件搬移决策：base target 幂等去重、`unique_name` `_N` 分派、
//! 目标目录计算。真正的 IO 搬移在 [`super::move_file`]。

use std::io;
use std::sync::Arc;

use camino::Utf8Path;
use sha2::{Digest, Sha512};
use tracing::debug;

use super::delta::{SourceDelta, record_failure};
use super::move_file::{do_move_or_dry_run, target_dir};
use crate::entities::backend::Backend;
use crate::entities::uri::Location;
use crate::usecases::config::config;

#[expect(
    clippy::too_many_arguments,
    reason = "单文件搬移上下文：src/source/output 三 Location + 两 backend + 已读字节 + mkdir_cache + dry_run/delta；折结构体在唯一调用点用一次反而冗余"
)]
pub(super) fn move_one(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    source: &Location,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    bytes: &[u8],
    mkdir_cache: &Arc<dashmap::DashSet<Location>>,
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
pub(super) enum TargetDecision {
    Duplicate,
    Fresh(Location),
    Exhausted,
}

/// 幂等 + `unique_name` 一体化决策：先探测 `base_target`；若存在且双侧 SHA-512 相等即
/// `Duplicate`，否则退到 `unique_name_from_index` 逐 `_N` 候选。size 快过滤（size 不同
/// 必内容不同）省一次 `open_read` 远端 RTT。
pub(super) fn dedupe_or_pick_target(
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

pub(super) fn bytes_hash_equal(
    backend: &Arc<dyn Backend>,
    loc: &Location,
    src_bytes: &[u8],
) -> io::Result<bool> {
    let mut reader = backend.open_read(loc)?;
    drain_and_hash_equal(&mut *reader, src_bytes)
}

/// `read_to_end` + SHA-512 双侧比对一体化：`?` Err arm 走 helper 内部让 caller
/// `bytes_hash_equal` 的调用站点无独立 `?` sub-region。
fn drain_and_hash_equal(reader: &mut dyn io::Read, src_bytes: &[u8]) -> io::Result<bool> {
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

fn log_deduplicated(src: &str, dst: &str) {
    debug!(
        feature = crate::usecases::report::FEATURE_MOVE_TEXT_SHOT,
        operation = "deduplicated",
        result = "ok",
        source = %src,
        target = %dst,
        "target already exists with identical content; src removed"
    );
}

/// 从 `_start` 起找空档 `_N` 候选。`max_attempts = N` 即 base 之外还有 N 个候选
/// （`_1..=_N`）。base 冲突由 caller 独立处理（走幂等或落此处 _1 起）。
pub(super) fn unique_name_from_index(
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
        match check_candidate_free(backend, &candidate_loc) {
            Ok(true) => return Ok(Some(candidate_loc)),
            Ok(false) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

fn check_candidate_free(backend: &Arc<dyn Backend>, loc: &Location) -> io::Result<bool> {
    backend.exists(loc).map(|exists| !exists)
}

/// 简化的 stem/ext 拆分；尾点（"a."）视作 stem="a." + ext=""，与 `Utf8Path::file_stem`/
/// `extension` 一致。
pub(super) fn split_stem_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

/// 计算 `src` 相对 `source` root 的子路径。`Utf8Path::strip_prefix` 不匹配时退回原 src
/// （walker 只 yield root 下 entry，不匹配是异常状况；保守返回完整路径让 target
/// 落在 `output/<full path>` 仍可定位不丢文件）。
pub(super) fn relative_to<'a>(src: &'a Utf8Path, source: &Utf8Path) -> &'a Utf8Path {
    src.strip_prefix(source).unwrap_or(src)
}
