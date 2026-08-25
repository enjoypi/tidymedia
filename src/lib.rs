#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod adapters;
mod entities;
mod frameworks;
mod usecases;

// ── Interface Adapters re-exports ──
// Backend Gateway 具体实现（LocalBackend/SmbBackend/...）位于 adapters；trait 定义
// 与值类型（Backend/Entry/...）保留在 entities。源代码依赖方向严格内向。
pub use adapters::backend::adb::{AdbBackend, AdbClient, AdbTarget};
pub use adapters::backend::factory::DefaultBackendFactory;
pub use adapters::backend::local::LocalBackend;
pub use adapters::backend::mtp::{MtpBackend, MtpClient, MtpMatch, MtpTarget};
pub use adapters::backend::smb::{SmbBackend, SmbClient, SmbTarget};
pub use adapters::cli::{Cli, Commands, run_cli};
pub use adapters::dispatch::{CommandResult, tidy, tidy_with};
pub use usecases::config::{CategoryDef, ClassifyConfig, FaceConfig, OcrConfig};
pub use usecases::cull::{CullReport, CulledEntry, GroupReport, ScoreBreakdown};
pub use usecases::move_text_shot::MoveTextShotReport;
pub use usecases::verify::{VerifyEntry, VerifyReport};
// doc(hidden) 观测口：content_diff 字节级纯函数（JPEG/PNG/BMFF 熵 hash + 旋转
// pHash）；供 tests/content_diff_deep.rs 在 release 实例直接构造 bytes 覆盖各分支。
#[doc(hidden)]
pub use usecases::verify::content_diff::{
    bmff_mdat_hash, entropy_hash, hash_rest, jpeg_entropy_hash, png_idat_hash,
    rotated_phash_similar,
};
#[doc(hidden)]
pub use usecases::verify::strip_source_root;
// doc(hidden) 观测口：tract 内部纯算法函数（真模型输出不会触发其防御分支，
// 如 SCRFD 恒 9 张、无 NaN）；供 tests/tract_decode_* 在 release 实例覆盖。
#[doc(hidden)]
pub use adapters::face::tract_eyestate::{
    decode as eyestate_decode, preprocess as eyestate_preprocess,
};
#[doc(hidden)]
pub use adapters::face::tract_facemesh::decode as facemesh_decode;
#[doc(hidden)]
pub use adapters::face::tract_mobilefacenet::decode as facenet_decode;
#[doc(hidden)]
pub use adapters::face::tract_scrfd::preprocess as scrfd_preprocess;
#[doc(hidden)]
pub use adapters::face::tract_scrfd_real::{ScaleMeta, decode_outputs, iou, nms};
#[doc(hidden)]
pub use adapters::ocr::tract_dbnet::{decide, preprocess as dbnet_preprocess};

// doc(hidden) 观测口：office epub/rtf 防御/变体分支（zip 条目遍历/容器存在性/
// 控制字边界）；供 tests/office_deep_* 在 release 实例覆盖。
#[doc(hidden)]
pub use entities::backend::partial_move_error;
#[doc(hidden)]
pub use entities::common::under_prefix;
#[doc(hidden)]
pub use entities::office::epub::{extract_text as epub_extract_text, parse as epub_parse};
#[doc(hidden)]
pub use entities::office::iwork::{
    extract_dates_from_plist as iwork_extract_dates_from_plist, parse as iwork_parse,
};
#[doc(hidden)]
pub use entities::office::odf::{
    extract_dates as odf_extract_dates, extract_text as odf_extract_text, parse as odf_parse,
    parse_odf_datetime as odf_parse_odf_datetime, scan_element_text as odf_scan_element_text,
};
#[doc(hidden)]
pub use entities::office::ooxml::{
    extract_dates as ooxml_extract_dates, extract_text as ooxml_extract_text, parse as ooxml_parse,
    parse_iso8601_to_epoch as ooxml_parse_iso8601_to_epoch,
    scan_element_text as ooxml_scan_element_text,
};
#[doc(hidden)]
pub use entities::office::rtf::{
    consume_control as rtf_consume_control, extract_text as rtf_extract_text, parse as rtf_parse,
    scan_int_after as rtf_scan_int_after, strip_rtf_into as rtf_strip_rtf_into,
};
#[doc(hidden)]
pub use usecases::cull::best_effort_remove_partial_dst;
// TEMP office_deep re-export（验证后移除；由主线程统一并入 lib.rs）
#[doc(hidden)]
pub use entities::office::cfb;
#[doc(hidden)]
pub use entities::office::mindmap_mm::{
    collect_text_attrs as mm_collect_text_attrs, extract_dates as mm_extract_dates,
    extract_text as mm_extract_text, parse as mm_parse,
};
#[doc(hidden)]
pub use entities::office::mindmap_zip;
#[doc(hidden)]
pub use entities::office::pdf::{
    collect_string_literals as pdf_collect_string_literals, extract_text as pdf_extract_text,
    extract_text_from_buf as pdf_extract_text_from_buf, parse as pdf_parse,
    scan_windows as pdf_scan_windows,
};
#[doc(hidden)]
pub use entities::office::scan::{strip_markup_into, truncate_at_boundary};
#[doc(hidden)]
pub use entities::office::text::{extract_text as text_extract_text, parse as text_parse};
#[doc(hidden)]
pub use entities::office::{extract_office_text, populate_office_dates};

// ── Entity re-exports ──
// `BackendFactory` Port 与 `Backend` 同层（entities/backend）；`DefaultBackendFactory`
// 是其唯一生产实现，置于 adapters。
pub use entities::backend::factory::BackendFactory;
pub use entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
pub use entities::common::Error;
pub use entities::common::Result;
pub use entities::media_time;
pub use entities::uri::{Location, ParseError as LocationParseError};

// ── Detector Factory Port + Default 装配 ──
// Port 在 usecases（推理 Port 内层规则），Default 装配在 frameworks（"decide-which
// -concrete-impl" 归最外层）。dispatch 通过 trait 消费，不直接引用 adapters::{face,ocr}。
pub use frameworks::detector::DefaultDetectorFactory;
pub use usecases::detector::DetectorFactory;

// Sidecar Gateway 的公开入口：协议解析在 adapters，路径名独立于 media_time 模块以
// 体现"外部数据格式适配器"职责。
pub use adapters::sidecar;

#[doc(hidden)]
pub use adapters::backend::fake::{FakeBackend, Op as FakeOp};

// ── Use Case Output Ports + Adapters ──
// TextDetector Port 在 usecases/ocr，具体实现 tract-onnx + Fake 在 adapters/ocr。
pub use adapters::ocr::build_detector;
#[doc(hidden)]
pub use adapters::ocr::fake::FakeTextDetector;
pub use usecases::ocr::TextDetector;

// DocumentClassifier Port（copy-doc/move-doc 内容分类）：Port 在 usecases/classify，
// tract embedding 实现 + Fake 在 adapters/classify。
pub use adapters::classify::build_classifier;
#[doc(hidden)]
pub use adapters::classify::fake::FakeDocumentClassifier;
pub use usecases::classify::{Classification, DocumentClassifier};

// Face Ports（cull 子命令用）：4 个 trait + `FaceDetection` DTO 在 usecases/face；
// 真实实现 + 4 个 build_* 装配函数 + Fake 在 adapters/face。
#[doc(hidden)]
pub use adapters::face::fake::{
    FakeEyeStateClassifier, FakeFaceDetector, FakeFaceEmbedder, FakeFaceMeshDetector,
};
pub use adapters::face::{
    build_eyestate_classifier, build_facemesh, build_facenet_embedder, build_scrfd_detector,
};
pub use usecases::face::{
    EyeStateClassifier, FaceDetection, FaceDetector, FaceEmbedder, FaceMeshDetector,
};

// uniffi 0.31 proc-macro 模式要求 setup_scaffolding! 出现在 crate 根；FFI 入口
// 与 DI 组装本体位于 frameworks/mobile（Clean Architecture Frameworks 层）。
#[cfg(feature = "android-app")]
uniffi::setup_scaffolding!();

/// CLI / FFI 启动钩子：把 frameworks 的 yaml/env loader 装到 `usecases::config`。
/// `bin/tidymedia.rs::main`、`frameworks/mobile.rs::*` 等入口 MUST 调一次；
/// 不调用则 [`usecases::config::config`] 取 [`usecases::config::Config::default`]。
/// 多次调用静默忽略后续（一次性 fn pointer）。
pub fn install_config_loader() {
    frameworks::config::install_global_loader();
}

/// 测试专用：重建全局 config（对应 `usecases::config::reset_config_loader`）。
/// `write_temp_config` 这类每测试换 yaml 的集成/单元测试在共享进程下必须先调用，
/// 再 `set_var` + `install_config_loader`，否则 OnceLock 沿用首个测试的配置。
#[doc(hidden)]
pub fn reset_config_loader() {
    usecases::config::reset_config_loader();
}
