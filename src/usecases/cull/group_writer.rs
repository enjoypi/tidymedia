//! group 目录写：算出 `output/<rel-dir>/group-NNN/` 路径，把最佳照片 `Backend::copy_file`
//! 复制为 `BEST_<basename>`，劣质副本 `Backend::rename` 搬到同目录，并写 `MANIFEST.json`。
//!
//! basename 冲突走 `unique_name_in_dir`（与 `move_text_shot` 同套路）。
//! MANIFEST.json 失败仅 warn 不中断（与 `JsonFileReportSink` 同哲学）。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tracing::warn;

use super::report::GroupReport;
use crate::entities::backend::Backend;
use crate::entities::uri::Location;
use crate::usecases::config::config;

const FEATURE: &str = "cull";
const BEST_PREFIX: &str = "BEST_";
const MANIFEST_NAME: &str = "MANIFEST.json";

/// 单个相似组的搬迁计划：最佳源 + 全部劣质源（按评分降序）。
pub(crate) struct GroupPlan<'a> {
    pub group_id: usize,
    pub best_source: &'a Location,
    pub best_source_backend: &'a Arc<dyn Backend>,
    /// `(source_loc, source_backend, score)` 三元组。
    pub culled: Vec<(&'a Location, &'a Arc<dyn Backend>, f32)>,
    pub best_score: f32,
    pub score_breakdown: super::report::ScoreBreakdown,
}

/// 把 `plan` 落盘：返填好的 `GroupReport`。`dry_run` 时只算路径不真搬。
///
/// # Errors
///
/// `mkdir_p` 失败、`copy_file` 失败、`rename` 失败均上抛由 caller 计入 report.failed。
pub(crate) fn write_group(
    plan: &GroupPlan<'_>,
    source_root: &Location,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    dry_run: bool,
    moved_counter: &mut usize,
) -> io::Result<GroupReport> {
    let group_dir = compute_group_dir(plan.best_source, source_root, output, plan.group_id);
    if !dry_run {
        output_backend.mkdir_p(&group_dir)?;
    }

    let best_basename = plan
        .best_source
        .path()
        .file_name()
        .ok_or_else(|| io::Error::other("best source has no file name"))?;
    let best_dst_name = format!("{BEST_PREFIX}{best_basename}");
    let best_dst = unique_name_in_dir(&group_dir, &best_dst_name, output_backend, dry_run)?;
    if !dry_run
        && let Err(e) = copy_file_cross_scheme(
            plan.best_source_backend,
            plan.best_source,
            output_backend,
            &best_dst,
        )
    {
        // 部分字节落盘后 Err 残留半截 dst 文件；重跑时 unique_name 跳到 BEST_x_1
        // 致重复堆积。best-effort 清理；若 remove 也失败（远端会话断开等）走 warn
        // 让用户能从日志发现残留累积，不静默吞 Err。
        if let Err(re) = output_backend.remove_file(&best_dst) {
            log_remove_err(&best_dst, &re);
        }
        return Err(e);
    }

    let mut culled_reports = Vec::with_capacity(plan.culled.len());
    for (src_loc, src_backend, score) in &plan.culled {
        let src_basename = src_loc
            .path()
            .file_name()
            .ok_or_else(|| io::Error::other("culled source has no file name"))?;
        let dst = unique_name_in_dir(&group_dir, src_basename, output_backend, dry_run)?;
        if !dry_run {
            move_file(src_backend, src_loc, output_backend, &dst)?;
            *moved_counter += 1;
        }
        culled_reports.push(super::report::CulledEntry {
            source_path: src_loc.display(),
            dest_path: dst.display(),
            score: *score,
        });
    }

    let report = GroupReport {
        group_id: plan.group_id,
        best_source: plan.best_source.display(),
        best_dest: best_dst.display(),
        culled: culled_reports,
        score_breakdown: plan.score_breakdown,
    };
    if !dry_run {
        write_manifest(&group_dir, output_backend, &report, plan.best_score);
    }
    Ok(report)
}

/// 算 `output/<best-rel-dir>/group-NNN/`。`best-rel-dir` 是最佳照片相对 source root 的目录。
fn compute_group_dir(
    best_source: &Location,
    source_root: &Location,
    output: &Location,
    group_id: usize,
) -> Location {
    let best_path = best_source.path();
    let rel_dir = best_path
        .strip_prefix(source_root.path())
        .ok()
        .and_then(camino::Utf8Path::parent)
        .map_or_else(Utf8PathBuf::new, Utf8PathBuf::from);
    let group_name = format!("group-{group_id:03}");
    let combined = if rel_dir.as_str().is_empty() {
        output.path().join(&group_name)
    } else {
        output.path().join(&rel_dir).join(&group_name)
    };
    output.with_path(combined)
}

/// basename 冲突走 `unique_name`：`a.jpg` 存在则 `a_1.jpg` / `a_2.jpg`。
/// `dry_run` 时直接返原 basename 不检 `exists`（避免 backend 调用）。
fn unique_name_in_dir(
    dir: &Location,
    file_name: &str,
    backend: &Arc<dyn Backend>,
    dry_run: bool,
) -> io::Result<Location> {
    let base_loc = dir.with_path(dir.path().join(file_name));
    if dry_run {
        return Ok(base_loc);
    }
    let (stem, ext) = split_stem_ext(file_name);
    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..=max_attempts {
        let candidate = if i == 0 {
            file_name.to_string()
        } else if ext.is_empty() {
            format!("{stem}_{i}")
        } else {
            format!("{stem}_{i}.{ext}")
        };
        let loc = dir.with_path(dir.path().join(&candidate));
        if !backend.exists(&loc)? {
            return Ok(loc);
        }
    }
    Err(io::Error::other(format!(
        "exhausted unique-name attempts in {}",
        dir.display()
    )))
}

fn split_stem_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

/// 跨 scheme copy 兜底：`Backend::copy_file` 实现按 scheme 校验 src/dst 必须同自家
/// scheme（`local.rs::copy_file_rejects_non_local_*_scheme` / `adb_ops_tests::copy_file_rejects_non_adb_scheme_on_either_side`
/// 等测试印证）。跨 scheme（local↔smb/adb/mtp）必走 stream：`src.open_read` +
/// `dst.open_write` + `io::copy` + `writer.finish`，保 caller 经 ? 上抛单点。
/// 同 scheme 仍走 `src_backend.copy_file` 以利用 backend 原生 copy（同卷 rename / SMB
/// `SRV_COPYCHUNK` 等）。
fn copy_file_cross_scheme(
    src_backend: &Arc<dyn Backend>,
    src: &Location,
    dst_backend: &Arc<dyn Backend>,
    dst: &Location,
) -> io::Result<u64> {
    if src_backend.scheme() == dst_backend.scheme() {
        return src_backend.copy_file(src, dst, false);
    }
    let mut reader = src_backend.open_read(src)?;
    let mut writer = dst_backend.open_write(dst, false)?;
    let bytes = io::copy(&mut reader, &mut writer)?;
    // finish 失败让 disk-full / 远端 commit Err 经 ? 上抛而非 Drop 静默吞
    // （MediaWriter::finish MUST ? 传播，CLAUDE.md 项目 gotcha 同口径）。
    writer.finish()?;
    Ok(bytes)
}

/// best-effort partial dst 清理失败时 warn：远端 SMB/ADB 会话断开让 remove 也 Err
/// 时让用户能从日志发现残留累积；tracing macro micro-region 集中此处 coverage(off)。
#[cfg_attr(coverage_nightly, coverage(off))]
fn log_remove_err(loc: &Location, e: &io::Error) {
    warn!(
        feature = FEATURE,
        operation = "remove_partial_dst",
        result = "warn",
        path = %loc.display(),
        error = %e,
        "best-effort partial dst removal failed; rerun may accumulate _N suffix residue"
    );
}

fn move_file(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    target_loc: &Location,
) -> io::Result<()> {
    if src_backend.scheme() == output_backend.scheme() && src_backend.scheme() == "local" {
        return src_backend.rename(src_loc, target_loc, false);
    }
    // 跨 scheme：copy + remove；copy 走 cross-scheme helper（直传 src/output backend
    // 内部按 scheme 分流：同 scheme→backend.copy_file 原子；跨 scheme→stream copy）。
    copy_file_cross_scheme(src_backend, src_loc, output_backend, target_loc)?;
    src_backend.remove_file(src_loc).map_err(|re| {
        io::Error::new(
            re.kind(),
            format!(
                "cull: copied {src} -> {dst} but cannot remove source: {re}",
                src = src_loc.display(),
                dst = target_loc.display(),
            ),
        )
    })
}

fn write_manifest(
    group_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    report: &GroupReport,
    best_score: f32,
) {
    #[derive(serde_derive::Serialize)]
    struct Manifest<'a> {
        group_id: usize,
        best: BestEntry<'a>,
        culled: &'a [super::report::CulledEntry],
        score_breakdown: super::report::ScoreBreakdown,
    }
    #[derive(serde_derive::Serialize)]
    struct BestEntry<'a> {
        src: &'a str,
        dst: &'a str,
        score: f32,
    }
    let manifest_loc = group_dir.with_path(group_dir.path().join(MANIFEST_NAME));
    let m = Manifest {
        group_id: report.group_id,
        best: BestEntry {
            src: &report.best_source,
            dst: &report.best_dest,
            score: best_score,
        },
        culled: &report.culled,
        score_breakdown: report.score_breakdown,
    };
    // Manifest 含 ScoreBreakdown 4 个 f32 字段；serde_json 对 NaN/Inf 返 Err 不写 null
    // （上游 face_scoring 异常路径 + EyeState/FaceMesh 模型 NaN 输出经 unwrap_or 之外
    // 路径传染时可触发）。manifest 是 best-effort（与 open_write Err warn 同口径）。
    let json = match serde_json::to_vec_pretty(&m) {
        Ok(j) => j,
        Err(e) => {
            warn!(
                feature = FEATURE,
                operation = "write_manifest",
                result = "serialize_error",
                error = %e,
                "MANIFEST.json serialize failed (NaN/Inf in score?); group skips manifest"
            );
            return;
        }
    };
    let mut writer = match output_backend.open_write(&manifest_loc, false) {
        Ok(w) => w,
        Err(e) => {
            warn!(
                feature = FEATURE,
                operation = "write_manifest",
                result = "open_error",
                error = %e,
                "MANIFEST.json open failed; group skips manifest"
            );
            return;
        }
    };
    if let Err(e) = writer.write_all(&json).and_then(|()| writer.finish()) {
        warn!(
            feature = FEATURE,
            operation = "write_manifest",
            result = "write_error",
            error = %e,
            "MANIFEST.json write failed"
        );
    }
}

#[cfg(test)]
#[path = "group_writer_tests.rs"]
mod tests;
