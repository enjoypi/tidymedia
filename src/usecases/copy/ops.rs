//! 单文件复制/移动操作：重复检测 → 媒体过滤 → 唯一命名 → fast-path rename 或流式拷贝。

use std::sync::Arc;

use tracing::debug;
use tracing::warn;

use super::naming::generate_unique_name;
use super::run::{CopyOpts, FEATURE_COPY};
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;

// `coverage(off)`：内含 duplicate 检测 + `if remove && !dry_run` 等多条 branch；
// lib_tidy / lib unit 两个 binary 的 LLVM monomorphization 副本无法同时让某一
// instance 覆盖所有 (T,F) 组合（dry_run / remove / include_non_media 三态笛卡尔
// 积太大，每个集成 binary 只走部分 case）。语义由现有 lib_tidy 集成测试
// （tidy_with_move_local_*、tidy_move_dry_run_with_duplicate_*、
// tidy_move_with_duplicate_removes_src_when_not_dry_run 等）联合断言。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn do_copy(
    src: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &mut Index,
    opts: &CopyOpts<'_>,
) -> common::Result<bool> {
    let src_loc = src.location().clone();
    let src_display = src.full_path.as_str();

    // 涉及物理删除/移动，判等用 SHA-512 杜绝 xxh3 碰撞误删。
    if let Some(dup) = output_index.exists(src, true)? {
        debug!(
            feature = FEATURE_COPY,
            operation = "detect_duplicate",
            result = "duplicate",
            source = src_display,
            duplicate = %dup,
            "source duplicates an existing file in output"
        );
        if opts.remove && !opts.dry_run {
            src.backend().remove_file(&src_loc)?;
        }
        return Ok(false);
    }

    if !opts.include_non_media && !src.is_media() {
        warn!(
            feature = FEATURE_COPY,
            operation = "filter_media",
            result = "skipped_non_media",
            source = src_display,
            "file is not an image or video (pass --include-non-media to copy anyway)"
        );
        return Ok(false);
    }

    if let Some((target_dir_loc, target_loc)) =
        generate_unique_name(src, output_dir, output_backend, opts.template)
    {
        if opts.dry_run {
            println!("\"{}\"\t\"{}\"", src_display, target_loc.display());
            return Ok(true);
        }

        output_backend.mkdir_p(&target_dir_loc)?;

        if opts.remove && src.backend().scheme() == "local" && output_backend.scheme() == "local" {
            // 同 LocalBackend + remove → 走 fs::rename fast-path：同卷 OS 原子完成，
            // 跨卷由 LocalBackend::rename 内部 fallback 到 fs::copy + fs::remove_file。
            // 不在此处用 dev() / GetVolumeInformationByHandleW 自己判同盘——OS 内核是
            // same-volume 判定的唯一权威源（识别 subst / junction / mount point /
            // bind mount / btrfs subvol 等所有边界），自己再判一遍既冗余又会漏边界。
            // mkparents=false：上方 mkdir_p 已建好父目录。
            src.backend().rename(&src_loc, &target_loc, false)?;
        } else {
            // 跨 backend 或 copy（remove=false）走 stream（mkparents=false 同上）。
            stream_copy(src, &target_loc, output_backend.as_ref())?;
            if opts.remove {
                src.backend().remove_file(&src_loc)?;
            }
        }
        println!("\"{}\"\t\"{}\"", src_display, target_loc.display());

        // target 是刚刚 copy 成功的产物，正常 FS 下必然存在；但 NFS/SMB 上偶发 ESTALE、
        // 反病毒/索引器抢占等场景仍可能让 metadata 失败。P0 §2 禁止生产代码 unwrap/expect，
        // 此处用 ? 传播 IO 错误，归入循环外的 failed 统计。
        _ = output_index.add(Info::open(&target_loc, Arc::clone(output_backend))?);

        Ok(true)
    } else {
        Err(common::Error::Io(std::io::Error::other(format!(
            "无法为\"{src_display}\"生成目标目录的文件名"
        ))))
    }
}

/// 用源 Info 的 backend 读 + 输出 backend 写。两个 backend 同一实例时与 `copy_file`
/// 等价；不同实例（跨 scheme）时仍工作。
///
/// 内部 4 个 `?` Err `分支（open_read` / `open_write` / `io::copy` / writer.finish）中，
/// `open_read` Err 已被 `do_copy_file_copy_fails_when_source_unreadable` 稳定覆盖；
/// 后三者在 `LocalBackend` 下要构造 disk-full / 父目录在 `mkdir_p` 后被外部抢删等
/// 不可稳定的场景，整函数随 CLAUDE.md「不可稳定触发」套路标 coverage(off)；
/// 剩余分支由 `FakeBackend` reader/writer error 注入的集成测试覆盖。
#[cfg_attr(coverage_nightly, coverage(off))]
fn stream_copy(src: &Info, target: &Location, out_be: &dyn Backend) -> common::Result<()> {
    let src_be = src.backend();
    let mut reader = src_be.open_read(src.location())?;
    let mut writer = out_be.open_write(target, false)?;
    std::io::copy(&mut reader, &mut writer)?;
    writer.finish()?;
    Ok(())
}
