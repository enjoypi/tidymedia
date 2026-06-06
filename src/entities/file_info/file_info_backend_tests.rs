//! `Info::open` 远端 backend 集成 + `create_time` warn/阈值 + `read_fill` 边界测试。
//! 从 `file_info_stream_tests.rs` 拆出避免单文件超限（P0 §6）。

use std::fs;
use std::io;

use sha2::Digest;

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

/// threshold=0 + 候选全空（EXIF 存在但无日期字段、`modified=None`）→ `secs==0`
/// 必须仍走 `fs_fallback`（= created），不得把 EPOCH 当判定结果返回。
/// 杀 `secs > 0` 被变异成 `>= 0`。注意两个构造要点：EXIF 必须存在（否则 let-else
/// 早返回走不到该行）；mtime 必须缺失（真实文件系统造不出，只能用
/// `FakeBackend::add_file_with_times`）。
#[test]
fn create_time_zero_threshold_without_candidates_uses_fs_fallback() {
    use std::time::{Duration, SystemTime};
    let fake = Arc::new(FakeBackend::new("fake"));
    let loc = Location::Local(camino::Utf8PathBuf::from("/in-memory/no-times.bin"));
    let created = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    fake.add_file_with_times(&loc, b"payload".to_vec(), None, Some(created));

    let mut info = super::Info::open(&loc, fake).unwrap();
    // 无日期字段的 EXIF：candidates_from_exif 产出为空，decision → None → secs=0
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    assert_eq!(info.create_time(0), created);
}

/// 缓冲区填满后 `read_fill` 不得再发起额外 read：用「数据尽即 Err」的 reader
/// 验证。`filled < buf.len()` 被变异成 `<=` 时会对空 slice 多读一次，把 EOF 后
/// 的 Err 误传播出来。
#[test]
fn read_fill_stops_exactly_at_buffer_capacity() {
    #[derive(Debug)]
    struct ErrAfterData {
        data: Vec<u8>,
        pos: usize,
    }
    impl io::Read for ErrAfterData {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() {
                return Err(io::Error::other("read past end"));
            }
            let n = (self.data.len() - self.pos).min(buf.len());
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }
    impl io::Seek for ErrAfterData {
        fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
            Ok(0)
        }
    }

    let mut r = ErrAfterData {
        data: vec![7u8; 16],
        pos: 0,
    };
    let mut buf = [0u8; 16];
    let n = super::read_fill(&mut r, &mut buf).expect("must not read past a full buffer");
    assert_eq!(n, 16);
}
