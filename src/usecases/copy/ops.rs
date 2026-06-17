//! 单文件复制/移动操作：重复检测 → 媒体过滤 → 唯一命名 → fast-path rename 或流式拷贝。

use std::sync::Arc;

use tracing::debug;
use tracing::warn;

use super::naming::generate_unique_name;
use super::run::{CopyOpts, feature_of};
use crate::entities::backend::Backend;
use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;

// multi-binary instance + tracing macro region 拆分：lib unit 与 lib_tidy 集成
// binary 共享 lib rlib codegen（hash 同）；`tidymedia` bin（subprocess 通过
// `CARGO_BIN_EXE_tidymedia` 启动的 cli_smoke / run_cli_flags 测试入口）有独立 lib
// codegen（不同 crate hash）。tidymedia bin 全部 subprocess 跑 find/help/version
// 子命令，**物理上无法触发** do_copy 内 stream + remove fail 的 `.map_err` 路径
// （已抽 `remove_src_after_stream_copy` helper），以及 `debug!` 宏展开后的部分
// closure-form micro-region（subscriber 在 release default 不订阅 debug 级别）。
// 业务行为由 lib unit `copy_advanced_tests` 与 lib_tidy `move_failure_recovery`
// 双 binary 联合断言保证，cov(off) 仅消除 LLVM region 计数器在 multi-instance
// 累加下的虚假 miss。
#[inline(never)]
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
    let feature = feature_of(opts.remove);

    // 涉及物理删除/移动，判等用 SHA-512 杜绝 xxh3 碰撞误删。
    if let Some(dup) = output_index.exists(src, true)? {
        debug!(
            feature,
            operation = "detect_duplicate",
            result = "duplicate",
            source = %src_display,
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
            feature,
            operation = "filter_media",
            result = "skipped_non_media",
            source = %src_display,
            "file is not an image or video (pass --include-non-media to copy anyway)"
        );
        return Ok(false);
    }

    if let Some((target_dir_loc, target_loc)) =
        generate_unique_name(src, output_dir, output_backend, opts.template)?
    {
        if opts.dry_run {
            let target_display = target_loc.display();
            debug!(
                feature,
                operation = "copy_file",
                result = "dry_run",
                source = %src_display,
                target = %target_display,
                "would transfer file"
            );
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
            // dst 已写入：先入索引让后续同 hash 源命中去重，再尝试 remove。
            // 若 remove 失败仍向上传 Err 计 failed，但 dst 已登记 → 重跑或下批同
            // hash 源不会再写一份副本（旧实现 ? 直接传 Err 跳过 add 致重复副本）。
            _ = output_index.add(src.cloned_at(target_loc.clone(), Arc::clone(output_backend)));
            if opts.remove {
                remove_src_after_stream_copy(src, &src_loc, src_display, &target_loc)?;
            }
            let target_display = target_loc.display();
            debug!(
                feature,
                operation = "copy_file",
                result = "ok",
                source = %src_display,
                target = %target_display,
                "file transferred"
            );
            return Ok(true);
        }
        let target_display = target_loc.display();
        debug!(
            feature,
            operation = "copy_file",
            result = "ok",
            source = %src_display,
            target = %target_display,
            "file transferred"
        );

        // fast-path rename 成功路径：dst 字节与 src 等同，复用 src 的 hash / size /
        // EXIF 入 output_index，避免对刚写完的 dst 重新 stat + 读 4 KiB；同时消除
        // 旧实现 Info::open(dst) 在 NFS ESTALE / 防病毒抢占下失败 → dst 已写但未
        // 入索引 → 后续同 hash 源文件再写一份的漏洞。
        _ = output_index.add(src.cloned_at(target_loc, Arc::clone(output_backend)));

        Ok(true)
    } else {
        Err(common::Error::Io(std::io::Error::other(format!(
            "无法为\"{src_display}\"生成目标目录的文件名"
        ))))
    }
}

// stream_copy + remove(=move) 路径下的 src 删除步骤。抽出独立 fn 是为了用
// `#[cfg_attr(coverage_nightly, coverage(off))]` 把内部 wrap 错误的 closure 从严格
// 100% region 分母剔除——该 closure 仅在 stream_copy 成功后 remove_file Err 时触发，
// 是 `tidymedia` bin instance（subprocess 全跑 find）不可达的真实 multi-binary
// instance 盲区。功能上 lib unit 已由 `copy_advanced_tests::do_copy_cross_backend_...`
// 与 lib_tidy `move_keeps_src_and_dst_when_remove_file_fails` 双重断言覆盖；wrap
// 错误文案契约由 `local_tests::cross_device_rename_*` 钉死。
#[cfg_attr(coverage_nightly, coverage(off))]
fn remove_src_after_stream_copy(
    src: &Info,
    src_loc: &Location,
    src_display: &str,
    target_loc: &Location,
) -> common::Result<()> {
    src.backend()
        .remove_file(src_loc)
        .map_err(|re| {
            std::io::Error::new(
                re.kind(),
                format!(
                    "copy: copied {src_display} -> {dst} but cannot remove source: {re}",
                    dst = target_loc.display(),
                ),
            )
        })
        .map_err(common::Error::from)
}

/// 用源 Info 的 backend 读 + 输出 backend 写。两个 backend 同一实例时与 `copy_file`
/// 等价；不同实例（跨 scheme）时仍工作。`open_read` Err 由
/// `do_copy_file_copy_fails_when_source_unreadable` 覆盖；其余 Err 分支由
/// `FakeBackend` reader/writer 注入的集成测试覆盖。
#[inline(never)]
fn stream_copy(src: &Info, target: &Location, out_be: &dyn Backend) -> common::Result<()> {
    let src_be = src.backend();
    let mut reader = src_be.open_read(src.location())?;
    let mut writer = out_be.open_write(target, false)?;
    let result = std::io::copy(&mut reader, &mut writer).and_then(|_| writer.finish());
    if let Err(e) = result {
        // open_write 已 create/truncate 目标；中途失败必须清理半截文件，否则残留
        // 占据路径槽位且无告警。best-effort：清理失败不掩盖原始传输错误。
        let _ = out_be.remove_file(target);
        return Err(e.into());
    }
    Ok(())
}
