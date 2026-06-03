//! `RealMtpClient` 占位 stub。仅在 `--features mtp-backend` 启用时编译，
//! 但当前实现 `RealMtpClient::new()` 直接返 `Unsupported`：跨平台 MTP 库
//! 尚未在 libmtp-rs / gphoto2-rs / 自接 rusb-PTP 三者之间定下选型。
//!
//! ## 待评估方案
//!
//! 1. **`libmtp-rs`** — 包 libmtp C 库。Linux 为主路径，Android NDK 可编但需
//!    打包 libusb + udev 规则。维护活跃但跨平台体验欠佳。
//! 2. **`gphoto2-rs`** — 包 libgphoto2。桌面平台覆盖好，Android 上更复杂。
//! 3. **自接 `rusb` 走 PTP/MTP 协议** — 跨平台 + Android NDK 友好（rusb 支持
//!    Android），但需自实现 ~30 个 OperationCode（GetStorageIDs / `GetObjectHandles`
//!    / `GetObjectInfo` / `GetObject` / `SendObjectInfo` / `SendObject` / `DeleteObject` ...）。
//!    工作量约 1500 行 + 协议状态机测试。
//!
//! ## 调度边界
//!
//! 调度层（[`super::MtpBackend::with_client`]）已经在测试中通过 `FakeMtpClient`
//! 完整验证；本 stub 仅占位 lib.rs `DefaultBackendFactory::for_location` 的
//! feature-gated 分支语义对称。

#![cfg_attr(coverage_nightly, coverage(off))]

use std::io;

pub struct RealMtpClient;

impl RealMtpClient {
    pub fn new() -> io::Result<Self> {
        Err(io::Error::other(
            "RealMtpClient is a stub; pick between libmtp-rs / gphoto2-rs / rusb-PTP before enabling",
        ))
    }
}
