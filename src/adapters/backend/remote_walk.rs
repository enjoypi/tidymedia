//! 远端递归遍历 / 递归建目录 / 统一错误日志。
//!
//! 从 `remote.rs` 拆出（原 514 行 → ≤300）：`walk_recursive` / `mkdir_recursive` /
//! `mkparent` / `log_mkparent_err` 是 [`RemoteBackend`](super::RemoteBackend) 的
//! `walk` / `mkdir_p` / `open_write` / `copy_file` 骨架；`map_and_log` 是全部远端
//! op 失败的结构化日志单点（非泛型，只编译一份）。

use std::collections::HashSet;
use std::io;
use std::sync::Arc;

use camino::Utf8Path;
use tracing::{debug, warn};

use crate::entities::backend::{Entry, EntryKind};

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

/// best-effort：父目录创建失败由随后的 write/copy 自身报错；R3 要求所有外部调用
/// 输出结构化日志，否则运维拿到的「写入 ENOENT」缺父目录创建失败上下文。
/// 抽独立 fn + `coverage(off)`：debug! 宏 closure-form micro-region 在 release
/// default subscriber 不订阅 debug 时永 0-hit，与 CLAUDE.md「tracing macro micro-region」
/// 套路一致；调用方的 `if let Err` 分支由 `open_write_mkparent_failure_swallowed_to_debug_log`
/// 等测试覆盖。
pub(super) fn log_mkparent_err<A: RemoteAdapter>(parent: &A::Target, e: &io::Error) {
    debug!(
        feature = "backend",
        scheme = A::scheme(),
        operation = "mkparent",
        path = %parent.path(),
        result = "error",
        error = %e,
        "mkparent best-effort failed; subsequent write will surface error"
    );
}

/// 递归扫描远端目录树，把所有 entry（含 Dir，与 `LocalBackend::walk` 行为对齐）收集到 `out`。
/// 单 list 失败即记 Err 不再下钻该子树；其余子树继续以"尽力而为"语义扫描。
///
/// **显式栈迭代（非递归）**：远端 Time Machine / rsync `--link-dest` 类深层备份
/// 目录树可达数千层，递归调用会消耗线程栈（每帧 KB 级 + Vec 分配）致 SIGSEGV，
/// 尤其在 `io_pool` 工作线程（默认 8 MiB）上更易触发。改用堆上 `Vec<Target>` worklist
/// 后深度只受堆内存约束，与 `LocalBackend::walk` 用 `ignore::WalkBuilder` (heap-based)
/// 的稳定性对齐。
pub(super) fn walk_recursive<A: RemoteAdapter>(
    adapter: &A,
    root: &A::Target,
    out: &mut Vec<io::Result<Entry>>,
) {
    // symlink/junction 环防护：远端 FS（ADB Android /sdcard、SMB DFS/junction）
    // 可能含循环 symlink 或挂载点回环，若 `kind_from_mode` 归类为 Dir → stack 无
    // 限 push 同一子树致 Vec<Target> 堆爆 OOM。visited 按 path 字符串 dedup 让环
    // 退化为 DAG。key 用 owned String（&Utf8Path 借用与 stack pop 生命周期冲突）；
    // deep tree 内存开销 O(unique dirs) 可接受，比 SIGSEGV/OOM 好数量级。
    // `LocalBackend::walk` 走 `ignore::WalkBuilder` 有 follow_links=false 缺省，此处对齐。
    let mut stack: Vec<A::Target> = Vec::with_capacity(16);
    let mut visited: HashSet<String> = HashSet::new();
    stack.push(root.clone());
    visited.insert(root.path().as_str().to_owned());
    while let Some(target) = stack.pop() {
        let listed = adapter
            .client()
            .list(&target)
            .map_err(|e| map_and_log(A::scheme(), "list", target.path(), A::map_error, e));
        let entries = match listed {
            Ok(v) => v,
            Err(e) => {
                out.push(Err(e));
                continue;
            }
        };
        for entry in entries {
            if entry.kind == EntryKind::Dir {
                match A::Target::from_location(&entry.location, adapter.ctx()) {
                    Ok(sub) => {
                        // 已 visit（环/symlink loop 或 backend 重复返子项）直接跳
                        // 过下钻；entry 本身仍 push 让 caller 得目录节点事件。
                        if visited.insert(sub.path().as_str().to_owned()) {
                            stack.push(sub);
                        }
                    }
                    Err(e) => {
                        // Dir entry 反向 from_location 失败：子树无法下钻，本目录条目
                        // 也跳过 Ok push——否则 caller 既收到 Err（已记 walker_errors）
                        // 又收到 Ok(Dir) 重复事件，且后者随后被 visit_location 静默
                        // 过滤掉，纯属噪声。
                        out.push(Err(e));
                        continue;
                    }
                }
            }
            out.push(Ok(entry));
        }
    }
}

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
