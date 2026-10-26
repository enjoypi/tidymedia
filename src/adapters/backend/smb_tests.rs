//! `SmbBackend` 测试：FakeRemoteClient<SmbTarget> 注入 + 调度逻辑 100% 覆盖。
//! 迁移到统一 FakeRemoteClient；协议特异断言通过 spy 读出。

use super::super::fake_remote::{FakeRemoteClient, RemoteFakeOp};
use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::*;
use crate::entities::backend::EntryKind;
use crate::entities::uri::Location;

// SMB 测试专用的 FakeRemoteClient：error_factory 把 PermissionDenied 转成
// 含 "EACCES" 文案的 Other error，从而触发 SmbAdapter::map_error。
type FakeClient = FakeRemoteClient<SmbTarget>;

fn fake_client() -> Arc<FakeClient> {
    Arc::new(FakeClient::with_error_factory(|k| match k {
        io::ErrorKind::PermissionDenied => io::Error::other("smb client returned EACCES"),
        other => io::Error::from(other),
    }))
}

fn smb(path: &str) -> Location {
    Location::Smb {
        user: Some("alice".into()),
        host: "nas".into(),
        port: Some(445),
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn backend_with(client: Arc<FakeClient>) -> SmbBackend {
    SmbBackend::with_client(client as Arc<dyn SmbClient>)
}

include!("smb_part1_tests.rs");
include!("smb_part2_tests.rs");
