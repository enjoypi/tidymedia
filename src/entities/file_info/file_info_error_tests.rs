//! `Info` 错误路径与 `pick_fs_fallback` 白盒测试（从 `file_info_tests.rs` 拆出）。

use std::fs;

use super::super::test_common as common;
use super::Info;

// 绝对路径直接跳过 canonicalize（full_path 内 is_absolute() 分支），随后 metadata() 失败。
// 触发 file_info.rs L71 metadata()? 的 Err region。
#[test]
fn info_from_absolute_missing_path_errors() {
    let err = Info::from("/definitely/missing/zzz_abs_path_xyz").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// 文件 metadata 可读但 read 不可（chmod 000），让 fast_hash 内 File::open 失败。
// 触发 file_info.rs L86 + L206/L209 的 Err region。
// 注意：测试结束前需恢复权限，否则 tempdir 清理会失败。
#[test]
#[cfg(unix)]
fn info_from_unreadable_file_errors() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.bin");
    fs::write(&path, b"non-empty content").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&path, perms.clone()).unwrap();

    let err = Info::from(path.to_str().unwrap()).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

    // 恢复权限，让 tempdir 在测试结束清理时能删除该文件
    perms.set_mode(0o644);
    fs::set_permissions(&path, perms).unwrap();
}

// Info 实例创建后立刻删除底层文件，再调 calc_full_hash → mmap 打开失败。
// 触发 file_info.rs L112 + L218/L219 的 Err region。
#[test]
fn calc_full_hash_errors_when_file_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vanishing.bin");
    fs::write(&path, b"contents that will vanish").unwrap();
    let info = Info::from(path.to_str().unwrap()).unwrap();
    fs::remove_file(&path).unwrap();
    let err = info.calc_full_hash().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// 同上，但走 secure_hash 路径。触发 L130 + L225/L226 的 Err region。
#[test]
fn secure_hash_errors_when_file_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vanishing2.bin");
    fs::write(&path, b"contents that will vanish 2").unwrap();
    let info = Info::from(path.to_str().unwrap()).unwrap();
    fs::remove_file(&path).unwrap();
    let err = info.secure_hash().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// 显式走 Info::open + LocalBackend，覆盖 L130 cache-hit True 分支。
#[test]
fn info_open_calc_full_hash_caches_on_second_call() {
    use super::super::super::adapters::backend::local::LocalBackend;
    use super::super::uri::Location;
    use camino::Utf8PathBuf;
    let path = Utf8PathBuf::from(common::DATA_LARGE);
    let info = Info::open(&Location::Local(path), LocalBackend::arc()).unwrap();
    let h1 = info.calc_full_hash().unwrap();
    let h2 = info.calc_full_hash().unwrap();
    assert_eq!(h1, h2);
}

// 显式走 Info::open + LocalBackend，覆盖 L147 cache-hit True 分支。
#[test]
fn info_open_secure_hash_caches_on_second_call() {
    use super::super::super::adapters::backend::local::LocalBackend;
    use super::super::uri::Location;
    use camino::Utf8PathBuf;
    let path = Utf8PathBuf::from(common::DATA_LARGE);
    let info = Info::open(&Location::Local(path), LocalBackend::arc()).unwrap();
    let s1 = info.secure_hash().unwrap();
    let s2 = info.secure_hash().unwrap();
    assert_eq!(s1, s2);
}

// 显式走 Info::open + LocalBackend，覆盖 L87 "is a directory" True 分支。
#[test]
fn info_open_rejects_directory_with_local_backend() {
    use super::super::super::adapters::backend::local::LocalBackend;
    use super::super::uri::Location;
    use camino::Utf8PathBuf;
    let err = Info::open(
        &Location::Local(Utf8PathBuf::from(common::DATA_DIR)),
        LocalBackend::arc(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("is a directory"), "got: {err}");
}

// 显式走 Info::open + LocalBackend，覆盖 L93 "is empty" True 分支。
#[test]
fn info_open_rejects_empty_file_with_local_backend() {
    use super::super::super::adapters::backend::local::LocalBackend;
    use super::super::uri::Location;
    use camino::Utf8PathBuf;
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let err = Info::open(&Location::Local(path), LocalBackend::arc()).unwrap_err();
    assert!(err.to_string().contains("is empty"), "got: {err}");
}

use std::time::Duration;
use std::time::SystemTime;

/// `pick_fs_fallback：modified` < created（罕见但合法）→ 取 modified。
#[test]
fn pick_fs_fallback_modified_smaller_than_created() {
    let m = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let c = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
    let got = super::pick_fs_fallback(Some(m), Some(c));
    assert_eq!(
        got.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        100
    );
}

/// `pick_fs_fallback：modified` ≥ created → 取 created。
#[test]
fn pick_fs_fallback_modified_ge_created() {
    let m = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
    let c = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let got = super::pick_fs_fallback(Some(m), Some(c));
    assert_eq!(
        got.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        100
    );
}

/// `pick_fs_fallback：created` 不可用（btime 缺失），只看 modified。
#[test]
fn pick_fs_fallback_created_none() {
    let m = SystemTime::UNIX_EPOCH + Duration::from_secs(50);
    let got = super::pick_fs_fallback(Some(m), None);
    assert_eq!(
        got.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        50
    );
}

/// `pick_fs_fallback：modified` 不可用（极端 fs），只看 created。
#[test]
fn pick_fs_fallback_modified_none() {
    let c = SystemTime::UNIX_EPOCH + Duration::from_secs(75);
    let got = super::pick_fs_fallback(None, Some(c));
    assert_eq!(
        got.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        75
    );
}

/// `pick_fs_fallback：两个时间都不可用` → `UNIX_EPOCH` 兜底。
#[test]
fn pick_fs_fallback_both_none() {
    let got = super::pick_fs_fallback(None, None);
    assert_eq!(got, SystemTime::UNIX_EPOCH);
}
