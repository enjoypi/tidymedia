//! move-text-shot 搬移执行：`do_move_or_dry_run` 分派 dry-run / 真跑，
//! `do_move_file` 走 rename fast-path 或 `stream_copy` + remove 单点。
//! target 目录拼接 `target_dir` 也在此（路径纯函数，与 IO 同模块便于局部推理）。

use std::io;
use std::sync::Arc;

use camino::Utf8Path;
use tracing::debug;

use super::delta::{SourceDelta, record_failure};
use crate::entities::backend::{Backend, partial_move_error, stream_copy};
use crate::entities::uri::Location;

#[expect(
    clippy::too_many_arguments,
    reason = "参数透传：src/output 两 backend + 三 Location + bytes + mkdir_cache + dry_run/delta"
)]
pub(super) fn do_move_or_dry_run(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    target_dir_loc: &Location,
    target_loc: &Location,
    bytes: &[u8],
    mkdir_cache: &Arc<dashmap::DashSet<Location>>,
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
fn log_move_ok(src: &str, dst: &str) {
    debug!(
        feature = crate::usecases::report::FEATURE_MOVE_TEXT_SHOT,
        operation = "move_file",
        result = "ok",
        source = %src,
        target = %dst,
        "file moved"
    );
}

fn log_move_dry_run(src: &str, dst: &str) {
    debug!(
        feature = crate::usecases::report::FEATURE_MOVE_TEXT_SHOT,
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
pub(super) fn do_move_file(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    target_dir_loc: &Location,
    target_loc: &Location,
    bytes: &[u8],
    mkdir_cache: &Arc<dashmap::DashSet<Location>>,
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
        partial_move_error(
            re.kind(),
            format!(
                "move_text_shot: copied {src} -> {dst} but cannot remove source: {re}",
                src = src_loc.display(),
                dst = target_loc.display(),
            ),
        )
    })
}

/// 拼接 `output / rel_dir`：`rel_dir` 为 `None` 或空 → 直接返 output。Local/远端 backend
/// 都通过 `Location::join_path` 单点扩展（CLAUDE.md「跨 scheme sibling 路径用
/// `Location::join_path` 单点」）。
pub(super) fn target_dir(output: &Location, rel_dir: Option<&Utf8Path>) -> Location {
    match rel_dir {
        None => output.clone(),
        Some(p) if p.as_str().is_empty() => output.clone(),
        Some(p) => output.join_path(p.as_str()),
    }
}
