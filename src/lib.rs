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
pub use usecases::cull::{CullReport, CulledEntry, GroupReport, ScoreBreakdown};
pub use usecases::move_text_shot::MoveTextShotReport;

// ── Entity re-exports ──
// `BackendFactory` Port 与 `Backend` 同层（entities/backend）；`DefaultBackendFactory`
// 是其唯一生产实现，置于 adapters。
pub use entities::backend::factory::BackendFactory;
pub use entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
pub use entities::common::Error;
pub use entities::common::Result;
pub use entities::media_time;
pub use entities::uri::{Location, ParseError as LocationParseError};

// Sidecar Gateway 的公开入口：协议解析在 adapters，路径名独立于 media_time 模块以
// 体现"外部数据格式适配器"职责。
pub use adapters::sidecar;

#[doc(hidden)]
pub use adapters::backend::fake::{FakeBackend, Op as FakeOp};

// ── Use Case Output Ports + Adapters ──
// TextDetector Port 在 usecases/ocr，具体实现 tract-onnx + Fake 在 adapters/ocr。
#[doc(hidden)]
pub use adapters::ocr::fake::FakeTextDetector;
pub use usecases::ocr::TextDetector;

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
