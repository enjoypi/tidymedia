//! [`BackendFactory`] Gateway 抽象：按 [`Location`] 装配 [`Backend`] 句柄。
//!
//! 与 [`Backend`] trait 同层（entities），让 usecases 仅依赖接口；具体装配
//! ([`adapters::backend::factory::DefaultBackendFactory`]) 在外层。

use std::sync::Arc;

use super::Backend;
use crate::entities::common::Result;
use crate::entities::uri::Location;

/// Backend 装配抽象：按 [`Location`] 构造对应的 [`Backend`] 句柄。
///
/// 生产路径走 `DefaultBackendFactory`：Local 直接给 `LocalBackend`，SMB / MTP
/// 在未启用对应 feature 时报 `Unsupported`。测试用 fake 实现注入
/// 覆盖跨 scheme 调度（见 `tests/lib_tidy.rs`）。
pub trait BackendFactory: Send + Sync {
    /// 根据 `loc` 的 scheme 构造并返回对应的 [`Backend`] 实现。
    ///
    /// # Errors
    ///
    /// 当对应 backend feature 未启用，或 backend 初始化（连接 / 认证）失败时返回 `Err`。
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>>;
}
