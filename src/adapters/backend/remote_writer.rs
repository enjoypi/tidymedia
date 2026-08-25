//! `RemoteBufferedWriter` 的 IO impl 与缓冲上限守卫。
//!
//! 从 `remote.rs` 拆出（原 514 行 → ≤300）：结构体仍定义在 `remote.rs`（字段私有，
//! 父模块 `open_write` 与测试可结构体字面量构造），本文件只放 `Debug` / `io::Write` /
//! `MediaWriter` impl 与 `check_buffer_size` 上限守卫（子模块可访问父模块私有字段）。

use std::io;

use crate::entities::backend::MediaWriter;

use super::map_and_log;
use super::{MAX_REMOTE_WRITE_BUFFER, RemoteAdapter, RemoteBufferedWriter, RemoteTarget};

impl<A: RemoteAdapter> std::fmt::Debug for RemoteBufferedWriter<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteBufferedWriter")
            .field("target", &self.target)
            .field("buffered_bytes", &self.buffer.len())
            // client 是 Arc<dyn RemoteClient<_>>，不 impl Debug，故用 finish_non_exhaustive
            .finish_non_exhaustive()
    }
}

/// buffer 上限守卫抽独立 helper：`Vec<u8>` 塞 2 GiB 才触上限，测试无法真分配，
/// 抽 `check_buffer_size(current, incoming)` 让上限判定纯逻辑可单测直触发。
pub(super) fn check_buffer_size(current: u64, incoming: u64) -> io::Result<()> {
    let new_len = current.saturating_add(incoming);
    if new_len > MAX_REMOTE_WRITE_BUFFER {
        return Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!(
                "remote write buffer exceeds {MAX_REMOTE_WRITE_BUFFER} bytes limit \
                 (client.write is not streaming; single-file byte total capped to avoid OOM)"
            ),
        ));
    }
    Ok(())
}

impl<A: RemoteAdapter> io::Write for RemoteBufferedWriter<A> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // 远端 client.write 一次性提交，buffer 必整体入堆。超 MAX_REMOTE_WRITE_BUFFER
        // 时 fail-fast 让 stream_copy 触发半截 dst 清理，比静默 OOM 崩进程可诊断
        // （Android FFI 2–4 GB RAM 场景尤其致命）。
        check_buffer_size(self.buffer.len() as u64, buf.len() as u64)?;
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<A: RemoteAdapter> MediaWriter for RemoteBufferedWriter<A> {
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.client
            .write(&self.target, &self.buffer)
            .map(|_| ())
            .map_err(|e| map_and_log(A::scheme(), "write", self.target.path(), A::map_error, e))
    }
}
