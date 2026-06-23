//! `cull` 主流程：scan → pHash 分组 → 4 模型印证评分 → `group_writer` 落盘。
//!
//! 5 阶段：
//! 1. **扫源**：walk 所有 source，按 `max_image_bytes` 跳过超大文件；逐张读字节 +
//!    `image::load_from_memory` 解码 + 计算 pHash + 灰度清晰度，**计算完即 drop 字节与
//!    解码图**——仅 `ScannedFile`（metadata + hash + sharpness）入 `Vec` 累计。
//!    旧实现把 `Arc<Vec<u8>>` + `Arc<RgbImage>` 也存进 `ScannedFile` 让 `pick_best` 阶段复用，
//!    但 `scan` 主循环会让整批图（2847 张 × ~17 MB ≈ 48 GB）驻留致 OOM；现按需在
//!    `analyze_image` 内对组内成员重读+重 decode（仅多图组承担二次开销，单图组直接跳过）。
//! 2. **pHash 分组**：Union-Find 按汉明距离 ≤ `phash_hamming_max` 分组
//! 3. **粗筛**：单图组（len=1）跳过：无可比较对象不搬迁不入 report
//! 4. **评分**：每组 per-image 跑 SCRFD → 5 点对齐 → `MobileFaceNet` 128 维 embedding +
//!    `FaceMesh` 468 点 EAR + `EyeState` 闭眼概率，`face_scoring::score_image` 出
//!    `ScoreBreakdown`；组内全部图算完后调 `identity_cluster::cluster_identities` 输出
//!    跨图身份簇日志；选 `breakdown.total` 最高者为 best
//! 5. **落盘**：调 `group_writer::write_group` 写 group 目录

use std::io;
use std::sync::Arc;

use image::RgbImage;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::crop::{crop_eye_around, crop_face_bbox, total_cmp_nan_as_neg_inf};
// 测试侧 `use super::*` 经此 re-export 拿到 `u32_from_f32_clamped` 微测试入口。
#[cfg(test)]
use super::crop::u32_from_f32_clamped;
use super::face_align;
use super::face_scoring;
use super::group_writer::{GroupPlan, write_group};
use super::identity_cluster;
use super::phash::{group_by_hash, phash};
use super::report::{CullReport, ScoreBreakdown};
use super::sharpness::laplacian_variance;
use super::util::{
    ensure_sources_outside_output, is_image, log_analyze_image, log_commit_group, log_cull_summary,
    log_identity_clusters, log_pick_best, log_scan_entry_ok, log_scan_source_complete,
    log_scan_source_start, read_all, record_failure,
};
use crate::entities::backend::factory::BackendFactory;
use crate::entities::backend::{Backend, Entry, EntryKind};
use crate::entities::common::{self, canonical_prefix, under_prefix};
use crate::entities::uri::Location;
use crate::usecases::config::{FaceConfig, config};
use crate::usecases::face::{
    EyeStateClassifier, FaceDetection, FaceDetector, FaceEmbedder, FaceMeshDetector,
};

/// 单文件扫描结果。仅持 metadata + phash + sharpness：`raw_bytes` 与 `decoded` 在
/// `scan_entry` 内算完即 drop，`analyze_image` 阶段按需重读重 decode（避免整批图驻留 OOM）。
struct ScannedFile {
    src_loc: Location,
    src_backend: Arc<dyn Backend>,
    source_root: Location,
    hash: u64,
    sharpness: f32,
}

/// 单张图 4 模型印证结果（faces 长度与其余 3 vec 一致：对齐失败/嵌入失败的 face 整体丢弃）。
struct ImageAnalysis {
    faces: Vec<FaceDetection>,
    embeddings: Vec<[f32; identity_cluster::EMBED_DIM]>,
    meshes: Vec<Vec<[f32; 3]>>,
    eye_states: Vec<(f32, f32)>,
}

/// 入口：5 阶段串联。
///
/// # Errors
///
/// - source ⊆ output 返 `InvalidInput`
/// - factory 构造 backend 失败、output `mkdir_p` 失败传播
/// - 单文件失败累计到 `report.failed`/`errors`
#[expect(
    clippy::too_many_arguments,
    reason = "4 detector + factory + sources/output + flags：注入侧主入口契约"
)]
pub fn cull(
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    factory: &dyn BackendFactory,
    sources: &[Location],
    output: &Location,
    dry_run: bool,
    phash_max_hamming: u8,
) -> common::Result<CullReport> {
    let face_cfg = &config().backend.face;
    let output_backend = factory.for_location(output)?;
    // canonical_prefix 让 symlink output（如 /tmp/out → /photos/cull_output）下
    // src 路径与 output prefix 字面可比；裸 display() 会让 under_prefix 误返 false
    // 致 move 模式把 output 内文件再次搬迁自身。
    let output_prefix = canonical_prefix(output);
    ensure_sources_outside_output(sources, &output_prefix)?;

    let mut report = CullReport {
        dry_run,
        ..CullReport::default()
    };
    if !dry_run {
        output_backend.mkdir_p(output)?;
    }

    let mut scanned: Vec<ScannedFile> = Vec::new();
    for source in sources {
        let src_backend = factory.for_location(source)?;
        scan_source(
            source,
            &src_backend,
            &output_prefix,
            face_cfg,
            &mut scanned,
            &mut report,
        );
    }
    // report.scanned 由 scan_source 增量累加触达的 source 文件数（含被识别为非媒体
    // 跳过、超大跳过、解码失败、IO 失败），口径与 CopyReport.scanned 一致；
    // 而 scanned vec 仅含成功解码的图（用于后续分组/评分），不是 report 的 scanned。

    let hashes: Vec<u64> = scanned.iter().map(|s| s.hash).collect();
    let groups = group_by_hash(&hashes, phash_max_hamming);

    // 阶段 A：模糊过滤串行（写 report.dropped_blurry）+ 收集合规多图组。
    let mut filtered: Vec<Vec<usize>> = Vec::new();
    for grp_indices in groups {
        if grp_indices.len() < 2 {
            continue;
        }
        // sharpness_min：多图组里剔除低于阈值的模糊图（yaml 注释承诺"单图组例外保留"，
        // len<2 已在上面 continue 跳过 → 此处过滤仅触发于多图组）。
        let (kept, dropped) = filter_blurry(&grp_indices, &scanned, face_cfg.sharpness_min);
        report.dropped_blurry += dropped;
        // 过滤后剩 < 2 张同样跳过：组失去比较意义。
        if kept.len() < 2 {
            continue;
        }
        filtered.push(kept);
    }

    // 阶段 B：组级 ONNX 评分并行（4 个 face trait 都是 Send+Sync，
    // tract `TypedRunnableModel::run(&self,...)` 支持 `&self` 并发调用）。
    let scored: Vec<ScoredGroup> = filtered
        .into_par_iter()
        .map(|indices| {
            let mut failures = Vec::new();
            let (best_idx, best_breakdown, breakdowns) = pick_best_for_group(
                &indices,
                &scanned,
                scrfd,
                facenet,
                facemesh,
                eyestate,
                face_cfg,
                &mut failures,
            );
            ScoredGroup {
                indices,
                best_idx,
                best_breakdown,
                breakdowns,
                failures,
            }
        })
        .collect();

    // 阶段 C：写盘串行（write_group 内含 mkdir_p / copy_file / unique_name 等 IO，
    // 串行让 group_id 单调分配 + report 顺序合并 + 远端 backend 不需并发安全契约）。
    let mut moved = 0_usize;
    let mut next_group_id = 1_usize;
    for mut sg in scored {
        // failures 先排走避免 commit_scored_group 借用 sg 时 partial move。
        let failures = std::mem::take(&mut sg.failures);
        for (path, err) in failures {
            record_failure(&mut report, path, &err);
        }
        commit_scored_group(
            sg,
            &scanned,
            output,
            &output_backend,
            dry_run,
            &mut next_group_id,
            &mut moved,
            &mut report,
        );
    }
    report.moved = moved;

    log_cull_summary(&report, dry_run);
    Ok(report)
}

/// 并行 ONNX 评分后由主线程串行写盘的中间态：保 indices 与 breakdowns 顺序对齐
/// `pick_best_for_group` 输出。
struct ScoredGroup {
    indices: Vec<usize>,
    best_idx: usize,
    best_breakdown: ScoreBreakdown,
    breakdowns: Vec<ScoreBreakdown>,
    failures: Vec<(String, io::Error)>,
}

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
fn commit_scored_group(
    sg: ScoredGroup,
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

/// 按 `sharpness_min` 阈值剔除多图组里的模糊图，返 `(剩余 indices, 剔除数)`。
/// NaN sharpness 视为合规（不剔除）：score 阶段会让 NaN 退化排序最低。
fn filter_blurry(indices: &[usize], scanned: &[ScannedFile], min: f32) -> (Vec<usize>, usize) {
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

/// 单文件并行处理结果：`None` = 非图静默跳；`Err` = 该计入 failed 的 IO/decode 错误。
type ScanOutcome = Option<Result<ScannedFile, (String, io::Error)>>;

fn scan_source(
    source: &Location,
    src_backend: &Arc<dyn Backend>,
    output_prefix: &str,
    face_cfg: &FaceConfig,
    out: &mut Vec<ScannedFile>,
    report: &mut CullReport,
) {
    log_scan_source_start(&source.display(), face_cfg.max_image_bytes);
    // 阶段 1：walker 串行收集合法 file entries（walker 本身轻量，IO 重头在单文件
    // read+decode）；同时把 walker_errors 与超大跳过即时记 failed。
    let mut entries: Vec<Entry> = Vec::new();
    for entry in src_backend.walk(source) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                // walker_errors 也算 scanned：与 CopyReport.scanned 同口径
                //（= indexed + skipped_empty + skipped_unreadable + walker_errors）。
                // 旧实现只增 failed 不增 scanned 让汇总日志 failed > scanned 误导排查。
                report.scanned += 1;
                record_failure(report, source.display(), &e);
                continue;
            }
        };
        if entry.kind != EntryKind::File {
            continue;
        }
        // under_prefix 命中 = 该文件位于 output 子树（同根归档场景），不算 source 触达。
        if under_prefix(&entry.location.display(), output_prefix) {
            continue;
        }
        // 触达 source 文件即计入 scanned（含后续被识别为非图/超大/解码失败/IO 失败的）；
        // 口径与 CopyReport.scanned 一致：walker 触达数而非成功入索引数。
        report.scanned += 1;
        if entry.size > face_cfg.max_image_bytes {
            let err = io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "cull: file {bytes} bytes exceeds backend.face.max_image_bytes={limit}",
                    bytes = entry.size,
                    limit = face_cfg.max_image_bytes,
                ),
            );
            record_failure(report, entry.location.display(), &err);
            continue;
        }
        entries.push(entry);
    }

    // 阶段 2：并行 read+decode+phash+sharpness。Backend: Send+Sync + scan_entry 是纯函数
    // 让 rayon worker 安全调用；OOM 已修（不再缓存图），N 核同时驻留 N 张图安全。
    let results: Vec<ScanOutcome> = entries
        .into_par_iter()
        .map(|e| scan_entry(src_backend, &e.location, source))
        .collect();

    // 阶段 3：主线程合并结果（report 是 &mut，并发改造仅限 IO/decode 阶段）。
    let total_entries = results.len();
    let mut valid = 0_usize;
    for r in results {
        match r {
            None => {}
            Some(Ok(file)) => {
                valid += 1;
                out.push(file);
            }
            Some(Err((path, err))) => record_failure(report, path, &err),
        }
    }
    log_scan_source_complete(&source.display(), valid, total_entries);
}

/// 单文件 scan：读字节 → MIME 嗅探 → decode → pHash + 灰度清晰度。
/// 纯函数（不持 `&mut report`）让 rayon worker 并发安全；错误以 `(path, err)` 形式
/// 上抛由主线程统一 `record_failure`。
fn scan_entry(
    src_backend: &Arc<dyn Backend>,
    location: &Location,
    source: &Location,
) -> ScanOutcome {
    let bytes = match read_all(src_backend, location) {
        Ok(b) => b,
        Err(e) => return Some(Err((location.display(), e))),
    };
    if !is_image(&bytes) {
        return None;
    }
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            return Some(Err((
                location.display(),
                io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")),
            )));
        }
    };
    let hash = phash(&img);
    // grayscale 接 GenericImageView，直接传 &img 避免 DynamicImage::ImageRgb8(img.clone())
    // 整图克隆（20 MiB 大图 peak RSS 三倍放大致批量扫 OOM 风险）。
    let luma = image::imageops::grayscale(&img);
    let sharp = laplacian_variance(&luma);
    // P0 §14 业务 debug：单图特征供 AI 分析分组前分布。
    log_scan_entry_ok(&location.display(), bytes.len() as u64, hash, sharp);
    // scan 阶段产出仅含 metadata + hash + sharpness：bytes 与 img 走出作用域立即释放，
    // 避免整批图驻留 OOM；analyze_image 对组内成员重读+重 decode（多图组承担）。
    Some(Ok(ScannedFile {
        src_loc: location.clone(),
        src_backend: src_backend.clone(),
        source_root: source.clone(),
        hash,
        sharpness: sharp,
    }))
}

/// 组内逐图跑 4 模型 + `face_scoring`，选 `breakdown.total` 最高者；同时调
/// `identity_cluster` 输出跨图身份簇日志（不影响选择，留作未来 per-identity 策略接入点）。
#[expect(
    clippy::too_many_arguments,
    reason = "组评分接 4 detector + cfg + failures：调用单点不再拆"
)]
fn pick_best_for_group(
    indices: &[usize],
    scanned: &[ScannedFile],
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    face_cfg: &FaceConfig,
    failures: &mut Vec<(String, io::Error)>,
) -> (usize, ScoreBreakdown, Vec<ScoreBreakdown>) {
    // 每个 indices 项总是有 breakdown：analyze_image 失败时退化为 sharpness-only
    // 计算（face_count=0 时 score_image 仅含 w_sharpness*sharpness 项）。这样 culled
    // 项的 score 字段也用 breakdown.total，与 best 的 score_breakdown.total 同口径，
    // 不再混用 sharpness 单分量（破坏 CulledEntry「综合评分」承诺）。
    let mut breakdowns: Vec<ScoreBreakdown> = Vec::with_capacity(indices.len());
    let mut per_image_embeddings: Vec<Vec<[f32; identity_cluster::EMBED_DIM]>> =
        Vec::with_capacity(indices.len());
    for &i in indices {
        let item = &scanned[i];
        let analysis = analyze_image(item, scrfd, facenet, facemesh, eyestate, face_cfg, failures);
        let (faces, meshes, eye_states, embeddings) = match analysis {
            Some(a) => (a.faces, a.meshes, a.eye_states, a.embeddings),
            None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        };
        per_image_embeddings.push(embeddings);
        breakdowns.push(face_scoring::score_image(
            item.sharpness,
            &faces,
            &meshes,
            &eye_states,
            face_cfg,
        ));
    }
    // TODO: per-identity 策略接入：clusters 当前仅产 debug 日志，pick_best 按全组
    // max(total) 选 best 不区分身份；若需「同人多张里选最佳 + 不同人各自保留」语义，
    // 在此按 clusters 分桶再于每桶内 max_by_key 取首张 → 当前 face_cosine_min 才有效。
    let clusters =
        identity_cluster::cluster_identities(&per_image_embeddings, face_cfg.face_cosine_min);
    log_identity_clusters(&clusters);

    // 选最高 total；NaN 视为 -∞ 让 NaN total 永远输给 finite 同分 → max_by 在
    // 全 finite 同分时 Rust 标准取末尾元素，配 `>` 严格比较保稳定（同 total 取首张
    // 即「先扫描的更优」直觉）；NaN 同 NaN 视为 Equal，返首个 NaN。
    // indices.len() >= 2（调用方保证）+ breakdowns 同长 → ok_or_else 兜底返
    // 第 0 项 breakdown 防 caller-contract 失守。
    let best_pos = breakdowns
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| total_cmp_nan_as_neg_inf(a.total, b.total))
        .map_or(0, |(p, _)| p);
    let best_idx = indices[best_pos];
    let best_breakdown = breakdowns[best_pos];
    // P0 §14 业务 debug：组规模 + 选中分数。
    log_pick_best(indices.len(), best_idx, best_breakdown.total);
    (best_idx, best_breakdown, breakdowns)
}

/// 单图 4 模型印证：按需重读字节 + 重 decode → `SCRFD` → 每脸 (`face_align` → `facenet`)
/// + (bbox crop → `facemesh`) + (eye crop → `eyestate`)。
///
/// 重读+重 decode 是 OOM 修复（scan 阶段不再缓存 `raw_bytes`/`decoded`）：仅多图组成员承担
/// 二次开销，单图组在 caller 已跳过。
///
/// `read_all`/`load_from_memory` Err 与 `SCRFD` Err 整图记 failure 返 None；
/// 单脸 `face_align`/`facenet` Err 整脸丢弃；`facemesh`/`eyestate` Err 退化为空 mesh
/// / 0 闭眼概率（不丢脸）。
fn analyze_image(
    item: &ScannedFile,
    scrfd: &dyn FaceDetector,
    facenet: &dyn FaceEmbedder,
    facemesh: &dyn FaceMeshDetector,
    eyestate: &dyn EyeStateClassifier,
    face_cfg: &FaceConfig,
    failures: &mut Vec<(String, io::Error)>,
) -> Option<ImageAnalysis> {
    let bytes = match read_all(&item.src_backend, &item.src_loc) {
        Ok(b) => b,
        Err(e) => {
            failures.push((item.src_loc.display(), e));
            return None;
        }
    };
    let decoded = match image::load_from_memory(&bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            failures.push((
                item.src_loc.display(),
                io::Error::new(io::ErrorKind::InvalidData, format!("decode image: {e}")),
            ));
            return None;
        }
    };
    let detections = match scrfd.detect_faces(item.src_loc.path(), &bytes) {
        Ok(f) => f,
        Err(e) => {
            failures.push((item.src_loc.display(), e));
            return None;
        }
    };
    let mut analysis = ImageAnalysis {
        faces: Vec::with_capacity(detections.len()),
        embeddings: Vec::with_capacity(detections.len()),
        meshes: Vec::with_capacity(detections.len()),
        eye_states: Vec::with_capacity(detections.len()),
    };
    for face in &detections {
        let Ok(aligned) = face_align::align_face(&decoded, &face.landmarks_5pt) else {
            continue;
        };
        let Ok(embedding) = facenet.embed_face(item.src_loc.path(), &aligned) else {
            continue;
        };
        let mesh = facemesh
            .detect_mesh(item.src_loc.path(), &crop_face_bbox(&decoded, face))
            .unwrap_or_default();
        let eye_pair = classify_eye_pair(
            item,
            &decoded,
            face,
            eyestate,
            face_cfg.eye_crop_radius_ratio,
        );
        analysis.faces.push(*face);
        analysis.embeddings.push(embedding);
        analysis.meshes.push(mesh);
        analysis.eye_states.push(eye_pair);
    }
    // P0 §14 ONNX 外部调用 debug：SCRFD 检测到的脸数 + 成功 embed/对齐的脸数（req=image，resp=faces）。
    log_analyze_image(
        &item.src_loc.display(),
        detections.len(),
        analysis.faces.len(),
    );
    Some(analysis)
}

/// 用 SCRFD 5 点的左/右眼坐标各 crop 一个方形眼区域调 EyeState，返左/右闭眼概率对。
fn classify_eye_pair(
    item: &ScannedFile,
    decoded: &RgbImage,
    face: &FaceDetection,
    eyestate: &dyn EyeStateClassifier,
    eye_crop_radius_ratio: f32,
) -> (f32, f32) {
    let bbox_h = (face.bbox[3] - face.bbox[1]).max(1.0);
    let radius = (bbox_h * eye_crop_radius_ratio).round();
    let left_crop = crop_eye_around(decoded, face.landmarks_5pt[0], radius);
    let right_crop = crop_eye_around(decoded, face.landmarks_5pt[1], radius);
    let left = eyestate
        .classify_eye(item.src_loc.path(), &left_crop)
        .unwrap_or(0.0);
    let right = eyestate
        .classify_eye(item.src_loc.path(), &right_crop)
        .unwrap_or(0.0);
    (left, right)
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
