//! MTP Backend：把 `mtp://device/storage/path` 形态的 [`Location`]
//! 转成 [`MtpClient`] trait 上的调用。client 自身可注入，真实库适配器留作未来 PR。
//!
//! ## 设备 / 存储模糊匹配
//! - URI 内的 `device` / `storage` 字段是用户书写的"名字"（如 `Pixel 8` /
//!   `Internal shared storage`），与 MTP 协议层的 device id / storage id 不一定一一对应。
//! - 通过 [`MtpMatch`] 控制匹配语义：`Exact` 要求严格相等；`Fuzzy` 由 client 自决（通常
//!   走 `contains` 等模糊算法）。
//! - 匹配策略由 [`MtpBackend::with_client`] 注入；上层（usecases / Registry）从配置读
//!   `backend.mtp.device_match` / `storage_match` 后传入。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::{Backend, Entry, MediaReader, MediaWriter, Metadata};
use crate::entities::uri::Location;

/// 匹配策略：与真实 MTP client 一起决定如何把 URI 内的 device/storage 名字
/// 落到协议层 id 上。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MtpMatch {
    Exact,
    Fuzzy,
}

/// MTP target 的最小可识别参数集。`MtpClient` 实现按此参数访问设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MtpTarget {
    pub device: String,
    pub storage: String,
    pub path: Utf8PathBuf,
    pub device_match: MtpMatch,
    pub storage_match: MtpMatch,
}

/// MTP 协议客户端抽象。`MtpBackend` 持 `Arc<dyn MtpClient>` 让真实库适配器与
/// fake 测试 client 等价替换。
pub trait MtpClient: Send + Sync + std::fmt::Debug {
    fn stat(&self, target: &MtpTarget) -> io::Result<Metadata>;
    fn list(&self, target: &MtpTarget) -> io::Result<Vec<Entry>>;
    fn read(&self, target: &MtpTarget) -> io::Result<Vec<u8>>;
    fn write(&self, target: &MtpTarget, data: &[u8]) -> io::Result<u64>;
    fn unlink(&self, target: &MtpTarget) -> io::Result<()>;
    fn mkdir(&self, target: &MtpTarget) -> io::Result<()>;
}

pub struct MtpBackend {
    client: Arc<dyn MtpClient>,
    device_match: MtpMatch,
    storage_match: MtpMatch,
}

impl MtpBackend {
    /// 真实库适配器未启用时的入口：返回 `Unsupported`。
    /// 真实 MTP 接入由 `Self::with_client`（未来 feature gated）落地。
    pub fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mtp-backend not enabled; rebuild with --features mtp-backend",
        ))
    }

    /// 注入自定义 client + 匹配策略。
    pub fn with_client(
        client: Arc<dyn MtpClient>,
        device_match: MtpMatch,
        storage_match: MtpMatch,
    ) -> Self {
        Self {
            client,
            device_match,
            storage_match,
        }
    }

    /// `Arc<dyn Backend>` 工厂：方便 Registry / Index::with_backend 等单元注入。
    pub fn arc_with_client(
        client: Arc<dyn MtpClient>,
        device_match: MtpMatch,
        storage_match: MtpMatch,
    ) -> Arc<dyn Backend> {
        Arc::new(Self::with_client(client, device_match, storage_match))
    }

    fn target(&self, loc: &Location) -> io::Result<MtpTarget> {
        build_target(loc, self.device_match, self.storage_match)
    }
}

impl std::fmt::Debug for MtpBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MtpBackend")
            .field("client", &self.client)
            .field("device_match", &self.device_match)
            .field("storage_match", &self.storage_match)
            .finish()
    }
}

impl Backend for MtpBackend {
    fn scheme(&self) -> &'static str {
        "mtp"
    }

    fn metadata(&self, loc: &Location) -> io::Result<Metadata> {
        let target = self.target(loc)?;
        self.client.stat(&target)
    }

    fn exists(&self, loc: &Location) -> io::Result<bool> {
        match self.metadata(loc) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn walk<'a>(
        &'a self,
        root: &Location,
    ) -> Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a> {
        let target = match self.target(root) {
            Ok(t) => t,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        let entries = match self.client.list(&target) {
            Ok(v) => v,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        Box::new(entries.into_iter().map(Ok))
    }

    fn open_read(&self, loc: &Location) -> io::Result<Box<dyn MediaReader>> {
        let target = self.target(loc)?;
        let bytes = self.client.read(&target)?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }

    fn open_write(
        &self,
        loc: &Location,
        mkparents: bool,
    ) -> io::Result<Box<dyn MediaWriter>> {
        let target = self.target(loc)?;
        if mkparents {
            if let Some(parent) = parent_target(&target) {
                let _ = self.client.mkdir(&parent);
            }
        }
        Ok(Box::new(MtpBufferedWriter {
            target,
            client: Arc::clone(&self.client),
            buffer: Vec::new(),
        }))
    }

    fn remove_file(&self, loc: &Location) -> io::Result<()> {
        let target = self.target(loc)?;
        self.client.unlink(&target)
    }

    fn mkdir_p(&self, loc: &Location) -> io::Result<()> {
        let target = self.target(loc)?;
        self.client.mkdir(&target)
    }

    fn read_to_string(&self, loc: &Location) -> io::Result<String> {
        let bytes = {
            let target = self.target(loc)?;
            self.client.read(&target)?
        };
        String::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn copy_file(
        &self,
        src: &Location,
        dst: &Location,
        mkparents: bool,
    ) -> io::Result<u64> {
        let src_target = self.target(src)?;
        let dst_target = self.target(dst)?;
        if mkparents {
            if let Some(p) = parent_target(&dst_target) {
                let _ = self.client.mkdir(&p);
            }
        }
        let bytes = self.client.read(&src_target)?;
        self.client.write(&dst_target, &bytes)
    }
}

/// MTP 写句柄：buffer 暂存 write 调用，`finish` 时一次性提交给 client。
struct MtpBufferedWriter {
    target: MtpTarget,
    client: Arc<dyn MtpClient>,
    buffer: Vec<u8>,
}

impl std::fmt::Debug for MtpBufferedWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MtpBufferedWriter")
            .field("target", &self.target)
            .field("buffered_bytes", &self.buffer.len())
            .finish()
    }
}

impl io::Write for MtpBufferedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MediaWriter for MtpBufferedWriter {
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.client.write(&self.target, &self.buffer).map(|_| ())
    }
}

fn build_target(
    loc: &Location,
    device_match: MtpMatch,
    storage_match: MtpMatch,
) -> io::Result<MtpTarget> {
    let Location::Mtp {
        device,
        storage,
        path,
    } = loc
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("MtpBackend cannot handle scheme {:?}", loc.scheme()),
        ));
    };
    Ok(MtpTarget {
        device: device.clone(),
        storage: storage.clone(),
        path: path.clone(),
        device_match,
        storage_match,
    })
}

/// 取目标在 storage 内的父目录。若 path 已在 storage 根，返回 None。
fn parent_target(t: &MtpTarget) -> Option<MtpTarget> {
    let parent = t.path.parent()?;
    if parent.as_str().is_empty() {
        return None;
    }
    Some(MtpTarget {
        path: parent.to_path_buf(),
        ..t.clone()
    })
}

#[cfg(test)]
#[path = "mtp_tests.rs"]
mod tests;
