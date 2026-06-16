//! `DefaultBackendFactory` 中真实 backend 装配路径。
//!
//! 每个 `build_*_backend` 都依赖对应 `*_real.rs` 中的 `Real*Client::new`，构造时需要
//! 真实 SMB / adb-server / Android 设备，CI 无法触发 → 与 `*_real.rs` 同遵 `_real.rs`
//! 命名约定让 cargo-llvm-cov `--ignore-filename-regex='_real\.rs$'` 把整文件排除。
//! 未启用对应 feature 时的 `Unsupported` 兜底分支保留在 `factory.rs` 内，由
//! `tests/lib_tidy.rs` 的 `tidy_rejects_*` 系列 100% 覆盖。

use std::sync::Arc;

use crate::entities::backend::Backend;
use crate::entities::common::Result;
use crate::entities::uri::Location;

#[cfg(feature = "smb-backend")]
pub(super) fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::smb::SmbBackend;
    use crate::adapters::backend::smb::SmbTarget;
    use crate::adapters::backend::smb::real::RealSmbClient;
    // 调度层保证只把 Location::Smb 路由到此处；let-else 仅在重构破坏不变量时触发。
    let Location::Smb {
        user,
        host,
        port,
        share,
        ..
    } = loc
    else {
        unreachable!("DefaultBackendFactory routes only Location::Smb here")
    };
    let target = SmbTarget {
        user: user.clone(),
        host: host.clone(),
        port: *port,
        share: share.clone(),
        path: camino::Utf8PathBuf::default(),
        password: std::env::var("SMB_PASSWORD").ok(),
        krb5_ccname: std::env::var("KRB5CCNAME").ok(),
    };
    let cfg = &crate::frameworks::config::config().backend.smb;
    // Error 已 #[from] io::Error，直接 `?` 自动转换。
    let client = RealSmbClient::new(&target, &cfg.default_user, &cfg.workgroup)?;
    Ok(SmbBackend::arc_with_client(Arc::new(client)))
}

#[cfg(feature = "mtp-backend")]
pub(super) fn build_mtp_backend(_loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::mtp::real::RealMtpClient;
    use crate::entities::common::Error;
    // stub 期 RealMtpClient::new() 必 Err，? 自然传播；OK 后续报 Unsupported 路径
    // 在真实 client 接入前是 dead code（已由 `tests/lib_tidy.rs` 的 e2e 覆盖 Err 传播）。
    let _client = RealMtpClient::new()?;
    Err(Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "mtp backend feature enabled but real client is not yet implemented",
    )))
}

#[cfg(feature = "adb-backend")]
pub(super) fn build_adb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use crate::adapters::backend::adb::AdbBackend;
    use crate::adapters::backend::adb::real::RealAdbClient;
    let Location::Adb { serial, .. } = loc else {
        unreachable!("DefaultBackendFactory routes only Location::Adb here")
    };
    let cfg = &crate::frameworks::config::config().backend.adb;
    let client = RealAdbClient::new(serial.clone(), &cfg.server_host, cfg.server_port)?;
    Ok(AdbBackend::arc_with_client(Arc::new(client)))
}
