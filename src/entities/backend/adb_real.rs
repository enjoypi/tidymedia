//! `RealAdbClient`：`adb_client` crate 适配器。
//!
//! 仅在 `--features adb-backend` 启用时编译。**整模块标 `#[cfg_attr(coverage_nightly,
//! coverage(off))]`**：真实 adb 调用需 adb-server 守护 + Android 设备才能稳定触发，
//! CI 无法覆盖。调度层（[`super::AdbBackend::with_client`] + FakeAdbClient）已覆盖
//! Target 翻译、错误映射、buffered writer 等全部 dispatch 分支。
//!
//! ## 线程安全
//!
//! `ADBServerDevice` 内部持 TCP socket，每次调用都串行 send / recv 命令；多线程
//! 共享同一个 device 句柄会撞 socket 状态。`RealAdbClient` 用 [`parking_lot::Mutex`]
//! 串行化所有调用，让 `Arc<dyn AdbClient>` 跨线程使用安全。
//!
//! ## 协议限制
//!
//! adb sync 协议原生只有 stat / list / pull / push。本适配器：
//! - `read` ← `device.pull(path, &mut buf)`
//! - `write` ← `device.push(&mut Cursor::new(data), path)`
//! - `unlink` / `mkdir` ← `device.shell_command("rm -f <quoted>")` / `mkdir -p <quoted>`
//!   shell 参数走 [`super::shell_quote`] 单引号转义防注入
//!
//! ## 未覆盖的能力
//!
//! - 多设备同时操作：`ADBServerDevice::autodetect` 仅在唯一在线设备时可用；
//!   多设备时上层 URI 必须带 serial
//! - timeout：`adb_client` 暂无显式 timeout API；`config.backend.adb.timeout_secs`
//!   保留为配置占位

#![cfg_attr(coverage_nightly, coverage(off))]

use std::io::{self, Cursor};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, SystemTime};

use adb_client::server_device::ADBServerDevice;
use adb_client::{ADBDeviceExt, ADBListItemType};
use parking_lot::Mutex;

use super::{shell_quote, AdbClient, AdbTarget};
use super::super::{Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

pub struct RealAdbClient {
    /// adb_client 的设备句柄。每次调用前 lock；adb sync 协议本身是串行的，
    /// 不存在丢锁后并发的语义。
    device: Mutex<ADBServerDevice>,
    /// `Some` 表示构造时带 serial；`None` 表示交给 client autodetect 唯一设备
    serial: Option<String>,
}

impl std::fmt::Debug for RealAdbClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealAdbClient")
            .field("serial", &self.serial)
            .finish()
    }
}

impl RealAdbClient {
    /// 按 serial（或 autodetect）+ adb-server 地址构造一个长连设备句柄。
    /// `server_host` / `server_port` 由 lib.rs 装配层从 `config()` 读取传入。
    pub fn new(
        serial: Option<String>,
        server_host: &str,
        server_port: u16,
    ) -> io::Result<Self> {
        let host: Ipv4Addr = server_host.parse().map_err(|e| {
            io::Error::other(format!("adb server_host parse: {e}"))
        })?;
        let addr = SocketAddrV4::new(host, server_port);
        let device = match &serial {
            Some(s) => ADBServerDevice::new(s.clone(), Some(addr)),
            None => ADBServerDevice::autodetect(Some(addr)),
        };
        Ok(Self {
            device: Mutex::new(device),
            serial,
        })
    }
}

impl AdbClient for RealAdbClient {
    fn stat(&self, target: &AdbTarget) -> io::Result<Metadata> {
        let path = target.path.as_str();
        // adb_client `stat` 接 `&dyn AsRef<str>` trait object，必须 `&&str` 二级借用；
        // clippy needless_borrow 在此 false-positive。
        #[allow(clippy::needless_borrows_for_generic_args)]
        let r = self
            .device
            .lock()
            .stat(&path)
            .map_err(|e| io::Error::other(format!("adb stat: {e}")))?;
        Ok(Metadata {
            size: u64::from(r.file_size),
            kind: kind_from_mode(r.file_perm),
            modified: Some(unix_secs(r.mod_time)),
            created: None,
        })
    }

    fn list(&self, target: &AdbTarget) -> io::Result<Vec<Entry>> {
        let path = target.path.as_str();
        #[allow(clippy::needless_borrows_for_generic_args)]
        let items = self
            .device
            .lock()
            .list(&path)
            .map_err(|e| io::Error::other(format!("adb list: {e}")))?;
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            let (kind, item) = match it {
                ADBListItemType::File(i) => (EntryKind::File, i),
                ADBListItemType::Directory(i) => (EntryKind::Dir, i),
                ADBListItemType::Symlink(i)
                | ADBListItemType::Fifo(i)
                | ADBListItemType::CharacterDevice(i)
                | ADBListItemType::BlockDevice(i)
                | ADBListItemType::Socket(i)
                | ADBListItemType::Other(i) => (EntryKind::Other, i),
            };
            if item.name == "." || item.name == ".." {
                continue;
            }
            let child_path = join_abs(&target.path, &item.name);
            out.push(Entry {
                location: Location::Adb {
                    serial: target.serial.clone(),
                    path: child_path,
                },
                size: u64::from(item.size),
                kind,
            });
        }
        Ok(out)
    }

    #[allow(clippy::needless_borrows_for_generic_args)]
    fn read(&self, target: &AdbTarget) -> io::Result<Vec<u8>> {
        let path = target.path.as_str();
        let mut buf: Vec<u8> = Vec::new();
        self.device
            .lock()
            .pull(&path, &mut buf)
            .map_err(|e| io::Error::other(format!("adb pull: {e}")))?;
        Ok(buf)
    }

    #[allow(clippy::needless_borrows_for_generic_args)]
    fn write(&self, target: &AdbTarget, data: &[u8]) -> io::Result<u64> {
        let path = target.path.as_str();
        let mut reader = Cursor::new(data);
        self.device
            .lock()
            .push(&mut reader, &path)
            .map_err(|e| io::Error::other(format!("adb push: {e}")))?;
        Ok(data.len() as u64)
    }

    fn unlink(&self, target: &AdbTarget) -> io::Result<()> {
        // adb sync 无原生 unlink：走 shell rm -f <quoted>。shell_quote 防注入。
        let cmd = format!("rm -f {}", shell_quote(target.path.as_str()));
        let mut stderr_buf: Vec<u8> = Vec::new();
        let code = self
            .device
            .lock()
            .shell_command(&cmd, None, Some(&mut stderr_buf))
            .map_err(|e| io::Error::other(format!("adb shell rm: {e}")))?;
        check_shell_exit(code, &stderr_buf, "rm")
    }

    fn mkdir(&self, target: &AdbTarget) -> io::Result<()> {
        let cmd = format!("mkdir -p {}", shell_quote(target.path.as_str()));
        let mut stderr_buf: Vec<u8> = Vec::new();
        let code = self
            .device
            .lock()
            .shell_command(&cmd, None, Some(&mut stderr_buf))
            .map_err(|e| io::Error::other(format!("adb shell mkdir: {e}")))?;
        check_shell_exit(code, &stderr_buf, "mkdir")
    }
}

/// stat 返回的 unix mode 高位包含文件类型（S_IFMT 段）。
/// 与 adb 协议 `ADBListItemType::from_mode_and_entry` 同套位运算。
fn kind_from_mode(mode: u32) -> EntryKind {
    match ((mode >> 13) & 0b111) as u8 {
        0b010 => EntryKind::Dir,
        0b100 => EntryKind::File,
        _ => EntryKind::Other,
    }
}

fn unix_secs(secs: u32) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(u64::from(secs))
}

/// 在已知绝对路径下拼接子项名。Utf8Path::join 默认会在父末尾补 `/`；
/// 这里直接构造字符串避免 windows 风格分隔符渗入测试断言。
fn join_abs(parent: &camino::Utf8Path, name: &str) -> camino::Utf8PathBuf {
    let p = parent.as_str().trim_end_matches('/');
    camino::Utf8PathBuf::from(format!("{p}/{name}"))
}

/// shell 命令的 exit code 检查：非 0 退出转成 io::Error::other 携带 stderr。
fn check_shell_exit(code: Option<u8>, stderr: &[u8], op: &str) -> io::Result<()> {
    let exit = code.unwrap_or(0);
    if exit == 0 {
        return Ok(());
    }
    let tail = String::from_utf8_lossy(stderr);
    Err(io::Error::other(format!(
        "adb shell {op} exit={exit}: {}",
        tail.trim()
    )))
}
