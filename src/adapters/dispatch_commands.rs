// 子命令 dispatch 拆分文件：原 dispatch.rs 420 行拆为两份各 ≤300。
// 本文件承接 `tidy_with` 各 arm 的透传执行（copy/move/copydoc/movedoc 合一
// `dispatch_copy_or_move` + find/move-text-shot/cull/verify）与公共 helper
// （`validate_template_arg` / `build_source` / `build_sources`）。
// 公开 API（`CommandResult` / `tidy` / `tidy_with`）留在 dispatch.rs；
// 本文件函数 `pub(super)` 仅父模块可见，签名与行为完全不变。

use crate::adapters::report_sink::JsonFileReportSink;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::{Error, Result};
use crate::entities::uri::Location;
use crate::usecases::config::validate_archive_template;
use crate::usecases::detector::DetectorFactory;
use crate::usecases::report::{Report, ReportSink};

use super::CommandResult;

// Copy / Move / CopyDoc / MoveDoc 区别仅 `remove` + `doc_only` 布尔组合；
// 提到此处避免四个 arm 同体重复。
#[expect(
    clippy::too_many_arguments,
    reason = "dispatch 单点接 CLI flag + factory + sources/output；折成结构体会让四个调用点也要先 Build 结构体"
)]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "dry_run/remove/include_non_media/doc_only 与 CLI flag 一一对应，收敛 enum 反让四个调用点更绕"
)]
pub(super) fn dispatch_copy_or_move(
    factory: &dyn BackendFactory,
    detectors: &dyn DetectorFactory,
    sources: Vec<Location>,
    output: Location,
    dry_run: bool,
    remove: bool,
    include_non_media: bool,
    doc_only: bool,
    archive_template: Option<&str>,
    report: Option<&str>,
) -> Result<CommandResult> {
    validate_template_arg(archive_template)?;
    // 分类器仅在 doc 命令且最终模板消费 {category} 时构造（快速失败：模板要
    // category 但模型未配 → InvalidInput）；构造是懒的，模型在首次分类才加载。
    let template = crate::usecases::resolved_template(archive_template, doc_only);
    let classifier = if doc_only && crate::usecases::template_needs_category(template) {
        let cfg = &crate::usecases::config::config().backend.classify;
        Some(crate::usecases::make_classify_provider(
            std::sync::Arc::from(detectors.build_document_classifier()?),
            cfg.max_text_bytes,
            cfg.score_min,
        ))
    } else {
        None
    };
    let src_pairs = build_sources(factory, sources)?;
    let out_pair = build_source(factory, output)?;
    let sink = report.map(JsonFileReportSink::new);
    let copy_report = crate::usecases::copy_with_sidecar(
        &src_pairs,
        out_pair,
        dry_run,
        remove,
        include_non_media,
        doc_only,
        archive_template,
        sink.as_ref().map(|s| s as &dyn ReportSink),
        // P3 sidecar 发现的依赖倒置注入点：adapters 协议解析进 usecases 流程。
        Some(crate::adapters::sidecar::discover_with_backend),
        classifier,
    )?;
    Ok(CommandResult::Copy(copy_report))
}

pub(super) fn dispatch_find(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    output: Option<Location>,
    secure: bool,
    report: Option<&str>,
) -> Result<CommandResult> {
    let src_pairs = build_sources(factory, sources)?;
    let out_pair = output.map(|loc| build_source(factory, loc)).transpose()?;
    let find_report = crate::usecases::find_duplicates(secure, src_pairs, out_pair.as_ref())?;
    // Find use case 当前不接 sink（report 由 dispatch 层捕获最终结构后落盘），
    // 与 Copy/Move 把 sink 当参数传给 use case 的形态不对称——find_duplicates
    // 无 progress 回调需求，单点写盘已够；若未来需要流式输出再改为同 Copy 形态。
    if let Some(path) = report {
        let sink = JsonFileReportSink::new(path);
        sink.write(&Report::Find(&find_report));
    }
    Ok(CommandResult::Find(find_report))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "由 Commands::MoveTextShot enum 解构 by-value 而来；usecase 接 &[]/& 借用"
)]
pub(super) fn dispatch_move_text_shot(
    factory: &dyn BackendFactory,
    detectors: &dyn DetectorFactory,
    sources: Vec<Location>,
    output: Location,
    dry_run: bool,
    report_path: Option<&str>,
) -> Result<CommandResult> {
    let detector = detectors.build_text_detector()?;
    let move_report =
        crate::usecases::move_text_shot(detector.as_ref(), factory, &sources, &output, dry_run)?;
    if let Some(path) = report_path {
        let sink = JsonFileReportSink::new(path);
        sink.write(&Report::MoveTextShot(&move_report));
    }
    Ok(CommandResult::MoveTextShot(move_report))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "由 Commands::Cull enum 解构 by-value 而来；usecase 接 &[]/& 借用"
)]
pub(super) fn dispatch_cull(
    factory: &dyn BackendFactory,
    detectors: &dyn DetectorFactory,
    sources: Vec<Location>,
    output: Location,
    dry_run: bool,
    phash_max: Option<u8>,
    report_path: Option<&str>,
) -> Result<CommandResult> {
    let scrfd = detectors.build_face_detector()?;
    let facenet = detectors.build_face_embedder()?;
    let facemesh = detectors.build_face_mesh()?;
    let eyestate = detectors.build_eye_state_classifier()?;
    // phash_hamming_max 默认从 face config 取；detector 构造已完成后仍需 config 值。
    let face_cfg = &crate::usecases::config::config().backend.face;
    let cull_report = crate::usecases::cull(
        scrfd.as_ref(),
        facenet.as_ref(),
        facemesh.as_ref(),
        eyestate.as_ref(),
        factory,
        &sources,
        &output,
        dry_run,
        phash_max.unwrap_or(face_cfg.phash_hamming_max),
    )?;
    if let Some(path) = report_path {
        let sink = JsonFileReportSink::new(path);
        sink.write(&Report::Cull(&cull_report));
    }
    Ok(CommandResult::Cull(cull_report))
}

pub(super) fn dispatch_verify(
    factory: &dyn BackendFactory,
    sources: Vec<Location>,
    output: Location,
    include_non_media: bool,
    exif_tsv: Option<&str>,
    phash_max: Option<u8>,
    report_path: Option<&str>,
) -> Result<CommandResult> {
    let src_pairs = build_sources(factory, sources)?;
    let out_pair = build_source(factory, output)?;
    let phash_max = phash_max.unwrap_or_else(|| {
        crate::usecases::config::config()
            .backend
            .face
            .phash_hamming_max
    });
    let verify_report = crate::usecases::verify(
        &src_pairs,
        &out_pair,
        phash_max,
        include_non_media,
        exif_tsv.map(std::path::Path::new),
    );
    if let Some(path) = report_path {
        let sink = JsonFileReportSink::new(path);
        sink.write(&Report::Verify(&verify_report));
    }
    Ok(CommandResult::Verify(verify_report))
}

// None 表示未传，跳过校验；Some(s) 时校验模板合法性。
fn validate_template_arg(template: Option<&str>) -> Result<()> {
    let Some(t) = template else {
        return Ok(());
    };
    validate_archive_template(t).map_err(|msg| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid --archive-template: {msg}"),
        ))
    })
}

fn build_source(factory: &dyn BackendFactory, loc: Location) -> Result<crate::usecases::Source> {
    let backend = factory.for_location(&loc)?;
    Ok((loc, backend))
}

fn build_sources(
    factory: &dyn BackendFactory,
    locs: Vec<Location>,
) -> Result<Vec<crate::usecases::Source>> {
    locs.into_iter()
        .map(|loc| build_source(factory, loc))
        .collect()
}
