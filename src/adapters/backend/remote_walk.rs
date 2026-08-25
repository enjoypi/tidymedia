//! 远端递归遍历 / 递归建目录 / 统一错误日志。
//!
//! 从 `remote.rs` 拆出（原 514 行 → ≤300）：`walk_recursive` / `mkdir_recursive` /
//! `mkparent` / `log_mkparent_err` 是 [`RemoteBackend`](super::RemoteBackend) 的
//! `walk` / `mkdir_p` / `open_write` / `copy_file` 骨架；`map_and_log` 是全部远端
//! op 失败的结构化日志单点（非泛型，只编译一份）。

use std::io;
use std::sync::Arc;

use camino::Utf8Path;
use tracing::{debug, warn};

use super::{RemoteAdapter, RemoteClient, RemoteTarget};

/// 统一记录远端 op 失败并套用协议级错误映射（R3：外部调用失败不静默）。
/// 非泛型：日志逻辑只编译一份，避免 monomorphization 覆盖率重复计数。
///
/// 级别分流：
/// - `NotFound` / `AlreadyExists` → `debug!`：`exists()` 探测、`mkdir_recursive`
///   容忍等高频预期态，info+ 级别下静默；
/// - 其余（`PermissionDenied` / `TimedOut` / IO 链路错误）→ `warn!`：用户感知
///   的远端业务错误，默认 info 级别下必须可见。
pub(super) fn map_and_log(
    scheme: &'static str,
    operation: &'static str,
    path: &Utf8Path,
    map: fn(io::Error) -> io::Error,
    e: io::Error,
) -> io::Error {
    let mapped = map(e);
    let err = mapped.to_string();
    let path = path.as_str();
    if matches!(
        mapped.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::AlreadyExists
    ) {
        debug!(
            feature = "backend",
            scheme,
            operation,
            path,
            result = "error",
            err,
            "remote op failed"
        );
    } else {
        warn!(
            feature = "backend",
            scheme,
            operation,
            path,
            result = "error",
            err,
            "remote op failed"
        );
    }
    mapped
}

pub(super) fn mkparent<A: RemoteAdapter>(
    target: &A::Target,
    client: &Arc<dyn RemoteClient<A::Target>>,
) {
    if let Some(parent) = target.parent()
        && let Err(e) = mkdir_recursive::<A>(&parent, client)
    {
        log_mkparent_err::<A>(&parent, &e);
    }
}

#[path = "remote_walk_phantom.rs"]
mod phantom;
#[doc(hidden)]
pub(crate) use self::phantom::{log_mkparent_err, walk_recursive};

/// 远端 mkdir-p：自底向上用 stat 找到第一个已存在的祖先，再自浅入深逐层 mkdir。
/// 远端协议的 mkdir 多为 POSIX 单层语义（父层缺失返回 ENOENT，如 pavao SMB），
/// 叶节点单次 mkdir 对 `{year}/{month}` 等多层 archive 模板必败。
/// `AlreadyExists` 容忍并发/重复创建；stat 的非 `NotFound` 错误（网络/权限）直接
/// 传播，避免在故障链路上盲目 mkdir。
pub(super) fn mkdir_recursive<A: RemoteAdapter>(
    target: &A::Target,
    client: &Arc<dyn RemoteClient<A::Target>>,
) -> io::Result<()> {
    let mut missing: Vec<A::Target> = Vec::new();
    let mut cur = Some(target.clone());
    while let Some(t) = cur {
        // pavao/adb_client 可能把"路径不存在"包成 Other("no such file")，必须经
        // A::map_error 归一成 NotFound 才能正确驱动自底向上的祖先扫描。
        match client.stat(&t).map_err(A::map_error) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                cur = t.parent();
                missing.push(t);
            }
            Err(e) => return Err(e),
        }
    }
    for t in missing.iter().rev() {
        // 并发或重复创建：原始 ErrorKind 已是 AlreadyExists 时不映射也对；但同样
        // 防御性走一遍映射，避免 Other("File exists") 之类文案被当硬错误传播。
        if let Err(e) = client.mkdir(t).map_err(A::map_error)
            && e.kind() != io::ErrorKind::AlreadyExists
        {
            return Err(e);
        }
    }
    Ok(())
}
