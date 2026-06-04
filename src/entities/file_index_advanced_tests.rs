//! `Index` 进阶/远端测试：`similar_files` / `exists_secure` / `visit_dir` 高级 / `visit_stats` / `visit_location` 多 backend。
//! 从 `file_index_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

use std::fs;

use camino::Utf8Path;
use tempfile::tempdir;

use super::super::file_info;
use super::super::test_common as common;
use super::Index;
use super::Info;

#[test]
fn similar_files_groups_collisions() {
    let mut index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    index.insert(common::DATA_SMALL_COPY).unwrap();
    let group = index
        .similar_files()
        .get(&common::DATA_SMALL_WYHASH)
        .expect("collision group present");
    assert_eq!(group.len(), 2);
    let small = file_info::full_path(common::DATA_SMALL).unwrap();
    let small_copy = file_info::full_path(common::DATA_SMALL_COPY).unwrap();
    assert!(group.contains(&small));
    assert!(group.contains(&small_copy));
    // 让 Utf8Path import 仍被使用
    let _ = Utf8Path::new(common::DATA_SMALL);
}

// exists(secure=true) 命中：覆盖 SHA-512 判等分支
#[test]
fn exists_secure_returns_some_for_duplicate() {
    let mut index = Index::new();
    index.insert(common::DATA_SMALL).unwrap();
    let dup = Info::from(common::DATA_SMALL_COPY).unwrap();
    let found = index
        .exists(&dup, true)
        .unwrap()
        .expect("duplicate must be detected via secure hash");
    assert_eq!(found, file_info::full_path(common::DATA_SMALL).unwrap());
}

// fast_hash 相同但 size 不同时，exists 必须 continue 不命中（覆盖 size != src.size 分支）
#[test]
fn exists_size_mismatch_skipped_even_with_fast_hash_collision() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.extend_from_slice(&[b'B'; 100]);
    fs::write(&b_path, &b).unwrap();

    let mut index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();
    let info_a = Info::from(a_path.to_str().unwrap()).unwrap();
    assert_eq!(info_a.fast_hash, info_b.fast_hash);
    assert_ne!(info_a.size, info_b.size);

    assert!(index.exists(&info_b, false).unwrap().is_none());
    assert!(index.exists(&info_b, true).unwrap().is_none());
}

// secure=true 时 index 中文件被删 → secure_hash IO Err 传播
#[test]
fn exists_secure_propagates_calc_hash_error_when_file_deleted() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.push(b'B');
    fs::write(&b_path, &b).unwrap();

    let mut index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();

    fs::remove_file(&a_path).unwrap();
    let err = index.exists(&info_b, true).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// secure=true 时 src 文件被删 → secure_hash IO Err 传播（右侧 ?）
#[test]
fn exists_secure_propagates_calc_hash_error_when_src_deleted() {
    let dir = tempdir().unwrap();
    let prefix = vec![0u8; 4096];

    let a_path = dir.path().join("a.bin");
    let mut a = prefix.clone();
    a.push(b'A');
    fs::write(&a_path, &a).unwrap();

    let b_path = dir.path().join("b.bin");
    let mut b = prefix;
    b.push(b'B');
    fs::write(&b_path, &b).unwrap();

    let mut index = Index::new();
    index.insert(a_path.to_str().unwrap()).unwrap();
    let info_b = Info::from(b_path.to_str().unwrap()).unwrap();
    fs::remove_file(&b_path).unwrap();
    let err = index.exists(&info_b, true).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

// visit_dir 必须不再尊重 .gitignore 规则（旧 ignore::Walk 默认会跳过被列入的文件）
#[test]
fn visit_dir_ignores_gitignore_rules() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.bin\n").unwrap();
    fs::write(dir.path().join("ignored.bin"), b"abcdef").unwrap();
    fs::write(dir.path().join("kept.bin"), b"012345").unwrap();

    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());

    let names: Vec<String> = index
        .files()
        .keys()
        .filter_map(|p| p.file_name().map(std::string::ToString::to_string))
        .collect();
    assert!(
        names.iter().any(|n| n == "ignored.bin"),
        "gitignore-listed file must still be indexed; got: {names:?}"
    );
    assert!(names.iter().any(|n| n == "kept.bin"));
}

// visit_dir 累计 skipped_empty；同时安装 warn 级 subscriber 让宏内字段表达式被求值
#[test]
fn visit_dir_counts_skipped_empty_with_warn_subscriber() {
    use tracing_subscriber::EnvFilter;
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("warn"))
        .with_writer(std::io::sink)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("empty.bin"), b"").unwrap();
        fs::write(dir.path().join("kept.bin"), b"abcdef").unwrap();
        let mut index = Index::new();
        index.visit_dir(dir.path().to_str().unwrap());
        let s = index.stats();
        assert_eq!(s.skipped_empty, 1);
        assert_eq!(index.files().len(), 1);
    });
}

// visit_dir 累计 skipped_unreadable（chmod 000）
#[test]
#[cfg(unix)]
fn visit_dir_counts_skipped_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let p = dir.path().join("locked.bin");
    fs::write(&p, b"abcdef").unwrap();
    let mut perms = fs::metadata(&p).unwrap().permissions();
    let original = perms.mode();
    perms.set_mode(0o000);
    fs::set_permissions(&p, perms.clone()).unwrap();

    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());

    // 恢复权限以便 tempdir 清理
    perms.set_mode(original);
    fs::set_permissions(&p, perms).unwrap();

    let s = index.stats();
    assert_eq!(s.skipped_unreadable, 1);
    assert_eq!(index.files().len(), 0);
}

// visit_dir 对不存在 root 计 walker_errors
#[test]
fn visit_dir_counts_walker_errors_on_missing_root() {
    let mut index = Index::new();
    index.visit_dir("/no/such/dir/zzz_walker_err_xyz");
    assert!(index.stats().walker_errors >= 1);
    assert_eq!(index.files().len(), 0);
}

#[test]
fn visit_stats_default_is_zero() {
    let s = super::VisitStats::default();
    assert_eq!(
        s,
        super::VisitStats {
            skipped_empty: 0,
            skipped_unreadable: 0,
            walker_errors: 0
        }
    );
}

#[test]
fn default_constructs_zero_state_index() {
    let index = Index::default();
    assert!(index.files().is_empty());
    assert_eq!(index.stats(), super::VisitStats::default());
}

// 同一 Index 承载两个不同 backend 的 visit_location 调用：
// - FakeBackend(smb)：放 1 个 1KiB 文件
// - FakeBackend(mtp)：放 1 个不同字节序列的 1KiB 文件
// 期望：files() 含两条记录，fast_hash 不同；Info 内部 backend 句柄各自归属。
#[test]
fn visit_location_accepts_multiple_backends_in_one_index() {
    use std::sync::Arc;

    use camino::Utf8PathBuf;

    use crate::adapters::backend::fake::FakeBackend;
    use crate::entities::backend::Backend;
    use crate::entities::uri::Location;

    let smb_root = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::new(),
    };
    let smb_file = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("a.jpg"),
    };
    let mtp_root = Location::Mtp {
        device: "Pixel".into(),
        storage: "Internal".into(),
        path: Utf8PathBuf::new(),
    };
    let mtp_file = Location::Mtp {
        device: "Pixel".into(),
        storage: "Internal".into(),
        path: Utf8PathBuf::from("b.jpg"),
    };

    let smb = Arc::new(FakeBackend::new("smb"));
    smb.add_dir(smb_root.clone());
    smb.add_file(smb_file.clone(), vec![0xAA; 1024]);

    let mtp = Arc::new(FakeBackend::new("mtp"));
    mtp.add_dir(mtp_root.clone());
    mtp.add_file(mtp_file.clone(), vec![0x55; 1024]);

    let mut index = Index::new();
    let smb_backend = Arc::clone(&smb) as Arc<dyn Backend>;
    let mtp_backend = Arc::clone(&mtp) as Arc<dyn Backend>;
    index.visit_location(&smb_root, &smb_backend);
    index.visit_location(&mtp_root, &mtp_backend);

    let files = index.files();
    assert_eq!(files.len(), 2, "both backends contributed one file each");

    let smb_key = Utf8PathBuf::from(smb_file.display());
    let mtp_key = Utf8PathBuf::from(mtp_file.display());
    assert!(files.contains_key(&smb_key));
    assert!(files.contains_key(&mtp_key));
    assert_ne!(
        files[&smb_key].fast_hash, files[&mtp_key].fast_hash,
        "distinct byte content should hash differently"
    );

    // 重新算 full_hash 必须走各自 Info 内部的 Arc<dyn Backend>——
    // 若实现退化为单 backend 共享，跨 scheme 的 open_read 会失败。
    assert!(files[&smb_key].calc_full_hash().is_ok());
    assert!(files[&mtp_key].calc_full_hash().is_ok());
}

// 文件名含非 UTF-8 字节时，Utf8PathBuf::from_path_buf 失败 → 计 walker_errors
#[test]
#[cfg(unix)]
fn visit_dir_counts_non_utf8_path() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = tempdir().unwrap();
    let bad = OsStr::from_bytes(&[b'a', 0xFF, 0xFE, b'.', b'b', b'i', b'n']);
    let p = dir.path().join(bad);
    fs::write(&p, b"abc").unwrap();

    let mut index = Index::new();
    index.visit_dir(dir.path().to_str().unwrap());
    assert!(
        index.stats().walker_errors >= 1,
        "non-UTF-8 path must bump walker_errors"
    );
    assert_eq!(index.files().len(), 0);
}
