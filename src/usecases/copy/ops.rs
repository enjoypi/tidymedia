//! 单文件复制/移动操作：重复检测 → 媒体过滤 → 唯一命名 → fast-path rename 或流式拷贝。

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use dashmap::DashSet;
use tracing::debug;

use super::run::CopyOpts;
use crate::entities::backend::{
    Backend, output_write_error, partial_move_error, stream_copy as backend_stream_copy,
};
use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use camino::Utf8Path;

#[path = "ops_phantom.rs"]
mod phantom;
#[doc(hidden)]
pub(crate) use self::phantom::do_copy;

pub(super) fn type_allowed(src: &Info, include_non_media: bool, doc_only: bool) -> bool {
    if doc_only {
        return src.is_office();
    }
    if include_non_media {
        return true;
    }
    src.is_media()
}

pub(super) fn count_type_filtered(source: &Index, opts: &CopyOpts<'_>) -> usize {
    source
        .iter()
        .filter(|kv| !type_allowed(kv.value(), opts.include_non_media, opts.doc_only))
        .count()
}

fn passes_type_filter(src: &Info, opts: &CopyOpts<'_>, feature: &'static str, disp: &str) -> bool {
    if type_allowed(src, opts.include_non_media, opts.doc_only) {
        return true;
    }
    debug!(
        feature,
        operation = "filter_media",
        result = "skipped_by_type",
        source = %disp,
        "file skipped by type filter"
    );
    false
}

fn remove_src_after_stream_copy(
    src: &Info,
    src_loc: &Location,
    src_display: &str,
    target_loc: &Location,
) -> common::Result<()> {
    src.backend()
        .remove_file(src_loc)
        .map_err(|re| {
            partial_move_error(
                re.kind(),
                format!(
                    "copy: copied {src_display} -> {dst} but cannot remove source: {re}",
                    dst = target_loc.display(),
                ),
            )
        })
        .map_err(common::Error::from)
}

/// 测试 shim：调原 [`do_copy`] 时按需构造空 `mkdir_cache`，并固定
/// `index_authoritative=false`（保守走 `backend.exists` 探测 = 旧行为）。生产
/// 路径走 `run_copy_loop` 持有的 loop 级缓存（命中已建目录跳过重复 `mkdir_p`
/// RTT）与 output 扫描 stats 推导的权威标记；测试桩不关心两者，让 12 处测试
/// 调用零改动。
#[cfg(test)]
pub(super) fn do_copy_with_default_cache(
    src: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &Index,
    opts: &CopyOpts<'_>,
) -> common::Result<bool> {
    let mc: DashSet<Location> = DashSet::new();
    do_copy(
        src,
        output_dir,
        output_backend,
        output_index,
        &mc,
        false,
        opts,
    )
}

/// 用源 Info 的 backend 读 + 输出 backend 写。经 `backend::stream_copy` 单点 helper
/// 完成（跨 scheme / 不同 backend 实例都走 1 MiB buffered stream + 三阶段闭合 +
///
/// 传 `prefer_native_copy=false`：ops.rs 无法从 `src.backend()` (Arc) 与 `out_be`
/// (`&dyn`) 判定是否同实例，保守走 stream 路径确保跨实例（Fake vs Local 同 scheme
/// `"local"`）语义正确；fast-path 优化由 `do_copy` 的 `supports_native_rename_to`
/// 分支承担。
#[inline(never)]
pub(super) fn refuse_self_duplicate(src: &Location, dup: &Utf8Path) -> common::Result<()> {
    let Location::Local(p) = src else {
        return Ok(());
    };
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(p), std::fs::canonicalize(dup))
        && a == b
    {
        return Err(common::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to remove {p}: it is the output duplicate {dup} itself"),
        )));
    }
    Ok(())
}

fn stream_copy(src: &Info, target: &Location, out_be: &dyn Backend) -> common::Result<()> {
    let src_be = src.backend();
    backend_stream_copy(src_be.as_ref(), src.location(), out_be, target, false)
        .map(drop)
        .map_err(output_write_error)?;
    Ok(())
}
