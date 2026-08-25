//! 单文件复制/移动操作：重复检测 → 媒体过滤 → 唯一命名 → fast-path rename 或流式拷贝。

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use dashmap::DashSet;
use tracing::warn;

use super::run::CopyOpts;
use crate::entities::backend::{Backend, partial_move_error, stream_copy as backend_stream_copy};
use crate::entities::common;
#[cfg(test)]
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;

#[path = "ops_phantom.rs"]
mod phantom;
#[doc(hidden)]
pub(crate) use self::phantom::do_copy;

// 落盘类型过滤，与 Exif::open_filtered 的解析短路同口径（三态）：doc_only 只留
// 文档族；否则 include_non_media 放行一切、默认只留媒体。从 do_copy 抽出
// （clippy too_many_lines）；`coverage(off)` 理由与 do_copy 相同（multi-binary
// instance 下 `warn!` micro-region 虚假 miss），业务由 `copy_doc_only_tests` /
// `copy_advanced_tests::include_non_media` 系列真测。
fn passes_type_filter(src: &Info, opts: &CopyOpts<'_>, feature: &'static str, disp: &str) -> bool {
    if opts.doc_only {
        if !src.is_office() {
            warn!(
                feature,
                operation = "filter_media",
                result = "skipped_non_document",
                source = %disp,
                "file is not a document (copy-doc/move-doc only archive document formats)"
            );
            return false;
        }
        return true;
    }
    if !opts.include_non_media && !src.is_media() {
        warn!(
            feature,
            operation = "filter_media",
            result = "skipped_non_media",
            source = %disp,
            "file is not an image or video (pass --include-non-media to copy anyway)"
        );
        return false;
    }
    true
}

// stream_copy + remove(=move) 路径下的 src 删除步骤。抽出独立 fn 是为了用
// `#[cfg_attr(coverage_nightly, coverage(off))]` 把内部 wrap 错误的 closure 从严格
// 100% region 分母剔除——该 closure 仅在 stream_copy 成功后 remove_file Err 时触发，
// 是 `tidymedia` bin instance（subprocess 全跑 find）不可达的真实 multi-binary
// instance 盲区。功能上 lib unit 已由 `copy_advanced_tests::do_copy_cross_backend_...`
// 与 lib_tidy `move_keeps_src_and_dst_when_remove_file_fails` 双重断言覆盖；wrap
// 错误文案契约由 `local_tests::cross_device_rename_*` 钉死。
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
/// 半截 dst 清理）。`cull/group_writer` 亦复用同 helper 消除重复实现。
///
/// 传 `prefer_native_copy=false`：ops.rs 无法从 `src.backend()` (Arc) 与 `out_be`
/// (`&dyn`) 判定是否同实例，保守走 stream 路径确保跨实例（Fake vs Local 同 scheme
/// `"local"`）语义正确；fast-path 优化由 `do_copy` 的 `supports_native_rename_to`
/// 分支承担。
#[inline(never)]
fn stream_copy(src: &Info, target: &Location, out_be: &dyn Backend) -> common::Result<()> {
    let src_be = src.backend();
    backend_stream_copy(src_be.as_ref(), src.location(), out_be, target, false).map(drop)?;
    Ok(())
}
