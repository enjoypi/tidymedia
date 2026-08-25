//! cull 落盘与阶段 A 过滤：
//! - `commit_scored_group`：组装 `GroupPlan` → `write_group` → 累计 report（主线程串行）
//! - `filter_multi_image_groups` / `filter_blurry`：单图组跳过 + `sharpness_min` 模糊剔除

use std::sync::Arc;

use super::group_writer::{GroupPlan, write_group};
use super::report::CullReport;
use super::scan::ScannedFile;
use super::util::{log_commit_group, record_failure};
use crate::entities::backend::Backend;
use crate::entities::uri::Location;

/// 把并行评分结果落盘：组装 `GroupPlan` → `write_group` → 累计 report。
/// 主线程串行调用让 `group_id` 单调分配 + report 顺序合并 + 远端 backend 不需并发契约。
#[expect(
    clippy::too_many_arguments,
    reason = "评分阶段已并行外置，本步骤仅做写盘 + report 合并：参数内聚一处"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "sg 调用后即丢弃，按值传让 plan 借用与 sg 字段生命周期同源"
)]
pub(super) fn commit_scored_group(
    sg: super::run::ScoredGroup,
    scanned: &[ScannedFile],
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    dry_run: bool,
    next_group_id: &mut usize,
    moved: &mut usize,
    report: &mut CullReport,
) {
    report.grouped += 1;
    let best = &scanned[sg.best_idx];
    // culled.score 用对应 breakdown.total（综合评分），与 best_breakdown.total 同口径，
    // 取代旧实现单字段 sharpness（CulledEntry 文档承诺综合评分）。
    let culled_refs: Vec<(&Location, &Arc<dyn Backend>, f32)> = sg
        .indices
        .iter()
        .enumerate()
        .filter(|&(_, &i)| i != sg.best_idx)
        .map(|(pos, &i)| {
            (
                &scanned[i].src_loc,
                &scanned[i].src_backend,
                sg.breakdowns[pos].total,
            )
        })
        .collect();
    let culled_len = culled_refs.len();
    let plan = GroupPlan {
        group_id: *next_group_id,
        best_source: &best.src_loc,
        best_source_backend: &best.src_backend,
        culled: culled_refs,
        best_score: sg.best_breakdown.total,
        score_breakdown: sg.best_breakdown,
    };
    // 计数搬到 Ok arm 内：write_group Err 时 groups 不 push，best_count/culled_count
    // 也必须保持原子（曾经在外提前累加，让 best_count != groups.len() 误导消费方）。
    // group_id 同口径：Err 时不消耗 ID，保 group-NNN 目录在 report.groups 序列连续，
    // 否则按 ID 枚举 group 目录的外部脚本出现缺号无法判断是失败遗留还是已处理。
    let group_id_attempt = *next_group_id;
    match write_group(
        &plan,
        &best.source_root,
        output,
        output_backend,
        dry_run,
        moved,
    ) {
        Ok(g) => {
            // P0 §14 业务 debug：写盘成功后落地的 group 计划（best 路径 + culled 数 + dry_run flag）。
            log_commit_group(
                group_id_attempt,
                &best.src_loc.display(),
                culled_len,
                dry_run,
            );
            report.best_count += 1;
            report.culled_count += culled_len;
            report.groups.push(g);
            *next_group_id += 1;
        }
        Err(e) => record_failure(report, best.src_loc.display(), &e),
    }
}

/// 阶段 A 抽出的单点：过滤单图组 + 应用 `sharpness_min` + 舍弃过滤后 <2 组。
/// 返 `(合规多图组, dropped_blurry 总数)`。抽独立 fn 让 branch counter 收敛到
/// 单 codegen instance（原本 inline 在 `cull_impl` 内被多个 e2e 测试触发 multi-instance
/// phantom miss），并让单元测试直接测「单元素组 skip」「filter 后 <2 skip」两 arm。
pub(super) fn filter_multi_image_groups(
    groups: Vec<Vec<usize>>,
    scanned: &[ScannedFile],
    sharpness_min: f32,
) -> (Vec<Vec<usize>>, usize) {
    let mut filtered: Vec<Vec<usize>> = Vec::new();
    let mut dropped_total: usize = 0;
    for grp_indices in groups {
        if grp_indices.len() < 2 {
            continue;
        }
        let (kept, dropped) = filter_blurry(&grp_indices, scanned, sharpness_min);
        dropped_total += dropped;
        if kept.len() < 2 {
            continue;
        }
        filtered.push(kept);
    }
    (filtered, dropped_total)
}

/// 按 `sharpness_min` 阈值剔除多图组里的模糊图，返 `(剩余 indices, 剔除数)`。
/// NaN sharpness 视为合规（不剔除）：score 阶段会让 NaN 退化排序最低。
pub(super) fn filter_blurry(
    indices: &[usize],
    scanned: &[ScannedFile],
    min: f32,
) -> (Vec<usize>, usize) {
    if !min.is_finite() || min <= 0.0 {
        return (indices.to_vec(), 0);
    }
    let mut kept: Vec<usize> = Vec::with_capacity(indices.len());
    let mut dropped = 0_usize;
    for &i in indices {
        if scanned[i].sharpness.is_finite() && scanned[i].sharpness < min {
            dropped += 1;
        } else {
            kept.push(i);
        }
    }
    (kept, dropped)
}
