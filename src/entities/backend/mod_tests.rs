//! `entities::backend::stream_copy` fallback 分支覆盖：
//! `prefer_native_copy=true` 但两 backend `scheme()` 不同 → `&&` short-circuit False
//! → 走 stream 路径而非 `src_be.copy_file`。此为「防御性双保险」的直测（caller
//! 违约传 prefer=true 但 backend 不同 scheme 时不至崩溃）。

use std::sync::Arc;

use camino::Utf8PathBuf;

use super::*;
use crate::adapters::backend::fake::FakeBackend;

fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "h".into(),
        port: None,
        share: "s".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn adb_loc(path: &str) -> Location {
    Location::Adb {
        serial: None,
        path: Utf8PathBuf::from(path),
    }
}

#[test]
fn stream_copy_prefer_native_but_scheme_mismatch_falls_through_to_stream() {
    let src_be = FakeBackend::new("smb");
    let src_loc = smb_loc("/a.bin");
    src_be.add_file(src_loc.clone(), b"payload".to_vec());
    let dst_be = FakeBackend::new("adb");
    let dst_loc = adb_loc("/b.bin");

    // prefer_native_copy=true 但 src.scheme()="smb" != dst.scheme()="adb"
    // → && 短路 False → 走 stream 路径（open_read / open_write / io::copy）
    let n = stream_copy(&src_be, &src_loc, &dst_be, &dst_loc, true).unwrap();
    assert_eq!(n, b"payload".len() as u64);
    // 验证 dst_be 已收到字节（stream 路径确实生效，而非 native copy）
    let mut r = dst_be.open_read(&dst_loc).unwrap();
    let mut got = Vec::new();
    std::io::copy(&mut r, &mut got).unwrap();
    assert_eq!(got, b"payload");
}

#[test]
fn stream_copy_prefer_native_and_same_scheme_takes_fast_path() {
    // 同实例 fake（scheme 匹配）+ prefer_native=true → 命中 L200-201 快路径
    let be = Arc::new(FakeBackend::new("smb"));
    let src_loc = smb_loc("/x.bin");
    let dst_loc = smb_loc("/y.bin");
    be.add_file(src_loc.clone(), b"hello".to_vec());
    let n = stream_copy(be.as_ref(), &src_loc, be.as_ref(), &dst_loc, true).unwrap();
    assert_eq!(n, b"hello".len() as u64);
}
