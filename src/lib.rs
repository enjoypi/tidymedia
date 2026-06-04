#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod adapters;
mod entities;
mod frameworks;
mod usecases;

// ── Interface Adapters re-exports ──
// Backend Gateway 具体实现（LocalBackend/SmbBackend/...）位于 adapters；trait 定义
// 与值类型（Backend/Entry/...）保留在 entities。源代码依赖方向严格内向。
pub use adapters::backend::adb::{AdbBackend, AdbClient, AdbTarget};
pub use adapters::backend::factory::{BackendFactory, DefaultBackendFactory};
pub use adapters::backend::local::LocalBackend;
pub use adapters::backend::mtp::{MtpBackend, MtpClient, MtpMatch, MtpTarget};
pub use adapters::backend::smb::{SmbBackend, SmbClient, SmbTarget};
pub use adapters::cli::{Cli, Commands, run_cli};
pub use adapters::dispatch::{CommandResult, tidy, tidy_with};

// ── Entity re-exports ──
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

// uniffi 0.31 proc-macro 模式要求 setup_scaffolding! 出现在 crate 根；FFI 入口
// 与 DI 组装本体位于 frameworks/mobile（Clean Architecture Frameworks 层）。
#[cfg(feature = "android-app")]
uniffi::setup_scaffolding!();
