//! `remote.rs` 进阶分支：`RemoteBufferedWriter` / `from_location_err` 系列 / root context 跳 mkdir。
//! 从 `remote_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::io;
use std::sync::Arc;

use super::test_helpers::{
    DummyAdapter, DummyClient, DummyTarget, backend_with_client, backend_with_from_loc_err,
    backend_with_root_ctx, loc,
};
use super::*;

#[test]
fn buffered_writer_write_and_flush() {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    let target = DummyTarget::new("/f");
    let mut w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: Vec::new(),
    };
    let n = io::Write::write(&mut w, b"abc").unwrap();
    assert_eq!(n, 3);
    io::Write::flush(&mut w).unwrap();
}

#[test]
fn buffered_writer_debug_shows_buffered_bytes() {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    let target = DummyTarget::new("/f");
    let w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: vec![0u8; 10],
    };
    let s = format!("{w:?}");
    assert!(s.contains("buffered_bytes"));
    assert!(s.contains("10"));
}

#[test]
fn buffered_writer_finish_writes_through() {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DummyClient::default());
    let target = DummyTarget::new("/f");
    let w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: b"data".to_vec(),
    };
    Box::new(w).finish().unwrap();
}

#[test]
fn buffered_writer_finish_write_err_propagates() {
    let c = DummyClient {
        write: Some(io::ErrorKind::TimedOut),
        ..Default::default()
    };
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(c);
    let target = DummyTarget::new("/f");
    let w = RemoteBufferedWriter::<DummyAdapter> {
        target,
        client,
        buffer: b"data".to_vec(),
    };
    let e = Box::new(w).finish().unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

// ── build_target Err（from_location 注入错误） ────────────────

#[test]
fn metadata_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.metadata(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn walk_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let mut iter = b.walk(&loc());
    let e = iter.next().unwrap().unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_read_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.open_read(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn open_write_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.open_write(&loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn remove_file_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.remove_file(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn mkdir_p_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.mkdir_p(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn read_to_string_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.read_to_string(&loc()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn copy_file_src_from_location_err_propagates() {
    let b = backend_with_from_loc_err(io::ErrorKind::InvalidInput);
    let e = b.copy_file(&loc(), &loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

// ── parent() == None 分支 ─────────────────────────────────────

#[test]
fn open_write_mkparents_with_root_skips_mkdir() {
    // from_location 返回根 target → parent() == None → mkdir 不调用
    let b = backend_with_root_ctx();
    let w = b.open_write(&loc(), true).unwrap();
    let s = format!("{w:?}");
    assert!(s.contains("RemoteBufferedWriter"));
}

#[test]
fn copy_file_mkparents_with_root_skips_mkdir() {
    let b = backend_with_root_ctx();
    let n = b.copy_file(&loc(), &loc(), true).unwrap();
    assert_eq!(n, 5);
}

// ── copy_file write Err ───────────────────────────────────────

#[test]
fn copy_file_write_err_propagates() {
    let c = DummyClient {
        write: Some(io::ErrorKind::TimedOut),
        ..Default::default()
    };
    let b = backend_with_client(c);
    let e = b.copy_file(&loc(), &loc(), false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
}

// ── RemoteAdapter::map_error 默认透传 ─────────────────────────

#[test]
fn default_map_error_passthrough() {
    // DummyAdapter 不覆写 map_error，应透传
    let e = io::Error::other("test");
    let mapped = DummyAdapter::map_error(e);
    assert_eq!(mapped.kind(), io::ErrorKind::Other);
}

/// mkparents=true 且 parent 已存在（`DummyClient::stat` 默认 Ok）时
/// `mkdir_recursive` 经 stat 短路，mkdir 不被调用。
#[test]
fn open_write_mkparents_skips_mkdir_when_parent_exists() {
    let client = Arc::new(DummyClient::default());
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client(
            Arc::clone(&client) as Arc<dyn RemoteClient<DummyTarget>>
        ),
    };
    b.open_write(&loc(), true).unwrap();
    let calls = client
        .mkdir_calls
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(calls, 0, "existing parent must not trigger mkdir");
}

/// mkparents=true 且 parent 缺失（stat 全 NotFound）时必须对 parent 链逐层 mkdir。
/// 杀「`mkparent` 整函数被替换成 `()`」：Dummy/Fake client 不强制父目录存在，
/// 仅靠结果断言无感知，必须数调用次数。`/dummy` 的 parent 链为 `/dummy` → `/`，
/// mkparent 从 parent(`/`) 起 → 恰一次 mkdir。
#[test]
fn open_write_mkparents_invokes_mkdir_when_parent_missing() {
    let client = Arc::new(DummyClient {
        stat: Some(io::ErrorKind::NotFound),
        ..Default::default()
    });
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client(
            Arc::clone(&client) as Arc<dyn RemoteClient<DummyTarget>>
        ),
    };
    b.open_write(&loc(), true).unwrap();
    let calls = client
        .mkdir_calls
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(calls, 1, "missing parent must trigger exactly one mkdir");
}
