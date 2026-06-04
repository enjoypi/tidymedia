//! `Info` 的流式 hash + `Info::open` 远端 backend 集成 + `create_time` warn 测试。
//! 从 `file_info_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::fs;
use std::io;
use std::io::Cursor;
use std::io::Read;

use sha2::Digest;
use xxhash_rust::xxh3;

use super::super::test_common as common;

/// 单次 read 限量到 32 字节的 reader，触发流式哈希的多次循环回边。
#[derive(Debug)]
struct ChunkedReader {
    data: Vec<u8>,
    pos: usize,
}
impl ChunkedReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}
impl io::Read for ChunkedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.data.len() - self.pos;
        let n = remaining.min(buf.len()).min(32);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
impl io::Seek for ChunkedReader {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "测试用小缓冲区，偏移量始终在 usize 范围内"
    )]
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match pos {
            io::SeekFrom::Start(p) => {
                self.pos = p as usize;
            }
            _ => return Err(io::Error::from(io::ErrorKind::Unsupported)),
        }
        Ok(self.pos as u64)
    }
}

/// 始终返回 `io::Error` 的 reader，用于覆盖 `read_fill` / full / secure 的 `?` 错误分支。
#[derive(Debug)]
struct FailingReader;
impl io::Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
    }
}
impl io::Seek for FailingReader {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

fn whole_file_bytes(path: &str) -> Vec<u8> {
    let mut f = fs::File::open(path).unwrap();
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).unwrap();
    buf
}

#[test]
fn fast_hash_stream_matches_path_version_small() {
    let bytes = whole_file_bytes(common::DATA_SMALL);
    let (path_n, path_w, path_x) = super::fast_hash(common::DATA_SMALL).unwrap();
    let mut r = Cursor::new(bytes);
    let (sn, sw, sx) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!((sn, sw, sx), (path_n, path_w, path_x));
}

#[test]
fn fast_hash_stream_matches_path_version_large() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (path_n, path_w, path_x) = super::fast_hash(common::DATA_LARGE).unwrap();
    let mut r = Cursor::new(bytes);
    let (sn, sw, sx) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!((sn, sw, sx), (path_n, path_w, path_x));
}

#[test]
fn fast_hash_stream_handles_chunked_reader() {
    // ChunkedReader 单次最多 32 字节：read_fill 必须循环多次填满 buffer
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (path_n, path_w, path_x) = super::fast_hash(common::DATA_LARGE).unwrap();
    let mut r = ChunkedReader::new(bytes);
    let (sn, sw, sx) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!((sn, sw, sx), (path_n, path_w, path_x));
}

#[test]
fn fast_hash_stream_empty_reader() {
    // 立即 EOF：read_fill 第一次 read 返回 0，break 退出
    let mut r = Cursor::new(Vec::<u8>::new());
    let (n, w, x) = super::fast_hash_stream(&mut r).unwrap();
    assert_eq!(n, 0);
    assert_eq!(w, wyhash::wyhash(&[], 0));
    assert_eq!(x, xxh3::xxh3_64(&[]));
}

#[test]
fn fast_hash_stream_io_error_propagates() {
    let mut r = FailingReader;
    let err = super::fast_hash_stream(&mut r).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn full_hash_stream_matches_path_version() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (_, path_full) = super::full_hash(common::DATA_LARGE).unwrap();
    let mut r = Cursor::new(bytes.clone());
    let (sn, sh) = super::full_hash_stream(&mut r).unwrap();
    assert_eq!(sh, path_full);
    assert_eq!(sn, bytes.len() as u64);
}

#[test]
fn full_hash_stream_chunked_matches() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (_, path_full) = super::full_hash(common::DATA_LARGE).unwrap();
    let mut r = ChunkedReader::new(bytes.clone());
    let (sn, sh) = super::full_hash_stream(&mut r).unwrap();
    assert_eq!(sh, path_full);
    assert_eq!(sn, bytes.len() as u64);
}

#[test]
fn full_hash_stream_empty_reader() {
    let mut r = Cursor::new(Vec::<u8>::new());
    let (n, h) = super::full_hash_stream(&mut r).unwrap();
    assert_eq!(n, 0);
    assert_eq!(h, xxh3::xxh3_64(&[]));
}

#[test]
fn full_hash_stream_io_error_propagates() {
    let mut r = FailingReader;
    assert_eq!(
        super::full_hash_stream(&mut r).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn secure_hash_stream_matches_path_version() {
    let bytes = whole_file_bytes(common::DATA_LARGE);
    let (_, path_secure) = super::secure_hash(common::DATA_LARGE).unwrap();
    let mut r = Cursor::new(bytes);
    let (_, sh) = super::secure_hash_stream(&mut r).unwrap();
    assert_eq!(sh, path_secure);
}

#[test]
fn secure_hash_stream_empty_reader() {
    let mut r = Cursor::new(Vec::<u8>::new());
    let (n, h) = super::secure_hash_stream(&mut r).unwrap();
    assert_eq!(n, 0);
    let expected = sha2::Sha512::digest(b"");
    assert_eq!(h, expected);
}

#[test]
fn secure_hash_stream_io_error_propagates() {
    let mut r = FailingReader;
    assert_eq!(
        super::secure_hash_stream(&mut r).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

// --- backend-aware Info::open 覆盖路径 ---

use super::super::uri::Location;
use crate::adapters::backend::fake::FakeBackend;
use camino::Utf8PathBuf;
use std::sync::Arc;

/// `Info::open` 走非 Local Location `时，full_path` 从 [`Location::display`] 派生。
/// 顺带覆盖 `calc_full_hash` / `secure_hash` 的 `FakeBackend` 路径。
#[test]
fn info_open_smb_location_derives_full_path_from_display() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let loc = Location::Smb {
        user: Some("alice".into()),
        host: "nas.local".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("dir/x.bin"),
    };
    fake.add_file(loc.clone(), b"hello-smb-content".to_vec());

    let info = super::Info::open(&loc, fake).unwrap();
    assert_eq!(info.size, 17);
    assert_eq!(info.full_path.as_str(), loc.display());
    // calc_full_hash 与 secure_hash 也走 FakeBackend
    let xxh = info.calc_full_hash().unwrap();
    assert_eq!(xxh, xxhash_rust::xxh3::xxh3_64(b"hello-smb-content"));
    let sha = info.secure_hash().unwrap();
    assert_eq!(sha, sha2::Sha512::digest(b"hello-smb-content"));
}

/// `FakeBackend::inject_reader_error` 让 `open_read` 成功但 read Err → `Info::open` 内
/// `fast_hash_stream(reader.as_mut())`? 的 Err 分支被触发。
#[test]
fn info_open_propagates_reader_error_from_fast_hash() {
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(Utf8PathBuf::from("/in-memory/x.bin"));
    fake.add_file(loc.clone(), vec![0u8; 64]);
    fake.inject_reader_error(loc.clone(), io::ErrorKind::Interrupted);

    let err = super::Info::open(&loc, fake).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

/// mtime 比 P0 早 > 30 天时 `create_time` 应发出 tracing warn 冲突告警。
///
/// 场景：给文件设置 mtime=2022-01-01，EXIF `DateTimeOriginal`=2024-01-01。
/// resolve 发现差距 > 30 天，push `MtimeMuchEarlierThanP0` 冲突，
/// `create_time` 内 `warn!` 被触发。
#[test]
fn create_time_emits_warn_when_mtime_much_earlier_than_p0() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tracing::field::Visit;
    use tracing_subscriber::Layer;
    use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;

    // local helper types.
    struct ConflictDetector;
    struct FieldVisitor(bool);
    impl Visit for FieldVisitor {
        fn record_debug(&mut self, _field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if format!("{value:?}").contains("MtimeMuchEarlierThanP0") {
                self.0 = true;
            }
        }
        fn record_str(&mut self, _field: &tracing::field::Field, value: &str) {
            if value.contains("MtimeMuchEarlierThanP0") {
                self.0 = true;
            }
        }
    }
    impl<S: tracing::Subscriber> Layer<S> for ConflictDetector {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() <= tracing::Level::WARN {
                let mut v = FieldVisitor(false);
                event.record(&mut v);
                if v.0 {
                    FIRED.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    // 2022-01-01T00:00:00Z
    const MTIME_EARLY: i64 = 1_640_995_200;
    // 2024-01-01T12:00:00Z — 差值 730 天 >> 30 天阈值
    const P0_SECS: u64 = 1_704_110_400;
    // AtomicBool 避免 Mutex，同时 'static 和 Send+Sync。
    static FIRED: AtomicBool = AtomicBool::new(false);
    FIRED.store(false, Ordering::SeqCst);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conflict.bin");
    fs::write(&path, b"conflict-test-content").unwrap();
    let ft = filetime::FileTime::from_unix_time(MTIME_EARLY, 0);
    filetime::set_file_mtime(&path, ft).unwrap();

    let mut info = super::Info::from(path.to_str().unwrap()).unwrap();
    info.set_exif(
        super::super::exif::Exif::with_mime("image/jpeg").with_date_time_original(P0_SECS),
    );

    let subscriber = tracing_subscriber::registry().with(ConflictDetector);
    tracing::subscriber::with_default(subscriber, || {
        let _ = info.create_time(946_684_800);
    });

    assert!(
        FIRED.load(Ordering::SeqCst),
        "expected warn with MtimeMuchEarlierThanP0 conflict"
    );
}

/// mtime 与 P0 差距小于阈值时不产生冲突告警。
#[test]
fn create_time_no_warn_when_mtime_close_to_p0() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tracing::field::Visit;
    use tracing_subscriber::Layer;
    use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;

    // local helper types.
    struct ConflictDetector2;
    struct FieldVisitor2(bool);
    impl Visit for FieldVisitor2 {
        fn record_debug(&mut self, _field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if format!("{value:?}").contains("MtimeMuchEarlierThanP0") {
                self.0 = true;
            }
        }
        fn record_str(&mut self, _field: &tracing::field::Field, value: &str) {
            if value.contains("MtimeMuchEarlierThanP0") {
                self.0 = true;
            }
        }
    }
    impl<S: tracing::Subscriber> Layer<S> for ConflictDetector2 {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() <= tracing::Level::WARN {
                let mut v = FieldVisitor2(false);
                event.record(&mut v);
                if v.0 {
                    FIRED.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    // P0 和 mtime 相差 1 天（< 30 天），不触发冲突
    const P0_SECS: u64 = 1_704_110_400;
    // 1704110400 - 86400 = 1704024000
    const MTIME_SECS: i64 = 1_704_024_000;
    static FIRED: AtomicBool = AtomicBool::new(false);
    FIRED.store(false, Ordering::SeqCst);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("noconflict.bin");
    fs::write(&path, b"no-conflict-content").unwrap();
    let ft = filetime::FileTime::from_unix_time(MTIME_SECS, 0);
    filetime::set_file_mtime(&path, ft).unwrap();

    let mut info = super::Info::from(path.to_str().unwrap()).unwrap();
    info.set_exif(
        super::super::exif::Exif::with_mime("image/jpeg").with_date_time_original(P0_SECS),
    );

    let subscriber = tracing_subscriber::registry().with(ConflictDetector2);
    tracing::subscriber::with_default(subscriber, || {
        let _ = info.create_time(946_684_800);
    });

    assert!(
        !FIRED.load(Ordering::SeqCst),
        "expected no MtimeMuchEarlierThanP0 conflict warn for 1-day diff"
    );
}

/// 用 `FakeBackend` 让 `calc_full_hash` 的 `full_hash_stream` `?` Err 分支被命中：
/// `Info::open` 走 `add_file` 的正常 reader 通过；之后注入 reader 错误，再调 `calc_full_hash`。
#[test]
fn calc_full_hash_propagates_reader_stream_error() {
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(Utf8PathBuf::from("/in-memory/y.bin"));
    fake.add_file(loc.clone(), vec![1u8; 64]);

    let info = super::Info::open(&loc, fake.clone()).unwrap();
    fake.inject_reader_error(loc, io::ErrorKind::ConnectionReset);
    let err = info.calc_full_hash().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
}

/// 同上，覆盖 `secure_hash` 的 `?` Err 分支。
#[test]
fn secure_hash_propagates_reader_stream_error() {
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(Utf8PathBuf::from("/in-memory/z.bin"));
    fake.add_file(loc.clone(), vec![2u8; 64]);

    let info = super::Info::open(&loc, fake.clone()).unwrap();
    fake.inject_reader_error(loc, io::ErrorKind::TimedOut);
    let err = info.secure_hash().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}
