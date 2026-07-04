//! 单文件复制/移动操作：重复检测 → 媒体过滤 → 唯一命名 → fast-path rename 或流式拷贝。

use std::collections::HashSet;
use std::sync::Arc;

use tracing::debug;
use tracing::warn;

use super::naming::generate_unique_name;
use super::run::CopyOpts;
use crate::entities::backend::{Backend, stream_copy as backend_stream_copy};
use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use crate::usecases::report::feature_of;

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
    mkdir_cache: &mut HashSet<Location>,
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
        generate_unique_name(src, output_dir, output_backend, opts.template, output_index)?
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
            // dry_run 也 add：generate_unique_name 后续调用会经 contains_target
            // 判定「本次已假设写过」，避免同 basename+同月+不同 hash 源被静默
            // 分派到相同 target 让 dry-run 报表 collision 漏报（真跑走 _N 分裂
            // 归档，tidy-verify 桶对账拿不到真跑口径）。
            //
            // Pre-populate src.secure_hash 让 cloned_at 的 lazy snapshot 携带 SHA-512：
            // 否则同 hash 后续 src 走 output_index.exists(secure=true) 会触发
            // f.secure_hash() → output_backend.open_read(target_loc)，dry-run 下
            // target 未真写返 NotFound 让 do_copy 假失败。
            src.secure_hash()?;
            _ = output_index.add(src.cloned_at(target_loc, Arc::clone(output_backend)));
            return Ok(true);
        }

        // mkdir 缓存：同 {year}/{month} 桶被 N 个文件命中时，N-1 次 mkdir_p 是远端
        // 2D 次 RTT 浪费（stat 链 + mkdir 链）。LocalBackend 本地 stat 廉价但仍走
        // syscall；远端 backend 是单点 RTT 杀手。失败不入缓存，下次重试。
        if !mkdir_cache.contains(&target_dir_loc) {
            output_backend.mkdir_p(&target_dir_loc)?;
            mkdir_cache.insert(target_dir_loc);
        }

        if opts.remove
            && src
                .backend()
                .supports_native_rename_to(output_backend.as_ref())
        {
            // Backend 声明支持原生原子 rename（LocalBackend override → 双端 local
            // 即 true；远端 backend 可 override 为 same-host + same-scheme）。
            // LocalBackend 内部走 fs::rename（同卷 OS 原子）+ CrossesDevices fallback
            // 到 fs::copy + fs::remove_file（半态用 wrap Err 文案标记）。
            // 不在此层用 dev() / GetVolumeInformationByHandleW 自己判同盘——OS 内核是
            // same-volume 判定的唯一权威源（识别 subst / junction / mount / bind /
            // btrfs subvol 等所有边界），自己再判一遍既冗余又会漏边界。
            // mkparents=false：上方 mkdir_p 已建好父目录。
            match src.backend().rename(&src_loc, &target_loc, false) {
                Ok(()) => {
                    let target_display = target_loc.display();
                    debug!(
                        feature,
                        operation = "copy_file",
                        result = "ok",
                        source = %src_display,
                        target = %target_display,
                        "file transferred"
                    );
                    // fast-path rename 成功：dst 字节与 src 等同，复用 src 的 hash /
                    // size / EXIF 入 output_index，避免对刚写完的 dst 重新 stat +
                    // 读 4 KiB；同时消除旧实现 Info::open(dst) 在 NFS ESTALE / 防病毒
                    // 抢占下失败让后续同 hash 源写重复副本的漏洞。
                    _ = output_index.add(src.cloned_at(target_loc, Arc::clone(output_backend)));
                    return Ok(true);
                }
                Err(e) => {
                    // 跨卷 fallback 半态：LocalBackend rename_or_fallback_with 内 fs::copy
                    // 成功但 fs::remove_file 失败会返 wrap 文案「cross-device rename:
                    // copied ... but cannot remove source」。dst 已在 fs 落地——即使
                    // 本次 do_copy 返 Err(failed+=1)，也 MUST 把 dst 登记入 output_index
                    // 让下一个同 hash src dedup 生效，否则 exists() 判 dst 存在走 _N
                    // 后缀又写一份物理副本。stream_copy 分支已通过「先 add 再 remove」
                    // 顺序修好此漏洞，fast-path 补齐同款救援。
                    if e.to_string().contains("cannot remove source") {
                        _ = output_index
                            .add(src.cloned_at(target_loc.clone(), Arc::clone(output_backend)));
                    }
                    return Err(e.into());
                }
            }
        }

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

/// 测试 shim：调原 [`do_copy`] 时按需构造空 `mkdir_cache`。生产路径走
/// `run_copy_loop` 持有的 loop 级缓存（命中已建目录跳过重复 `mkdir_p` RTT），
/// 测试桩不关心缓存复用，每次空 set 入参等价旧行为；既保留 `mkdir_cache` 参数
/// 强制每次调用决策（不退化为隐式默认），又让 12 处测试调用零改动。
#[cfg(test)]
pub(super) fn do_copy_with_default_cache(
    src: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &mut Index,
    opts: &CopyOpts<'_>,
) -> common::Result<bool> {
    let mut mc: HashSet<Location> = HashSet::new();
    do_copy(src, output_dir, output_backend, output_index, &mut mc, opts)
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
