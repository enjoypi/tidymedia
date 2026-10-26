/// metadata 失败（PermissionDenied / 网络错误等非 NotFound）必须传播原 Err，
/// 不被吞成"not a directory"——曾用 `is_ok_and` 把 IO 错误一起吞掉致排查方向被误导。
#[test]
fn find_duplicates_propagates_non_notfound_metadata_error() {
    use crate::adapters::backend::fake::FakeBackend;

    let fake = Arc::new(FakeBackend::new("smb"));
    let remote_dir = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("out"),
    };
    fake.add_dir(remote_dir.clone());
    fake.inject_error(
        remote_dir.clone(),
        crate::adapters::backend::fake::Op::Metadata,
        std::io::ErrorKind::PermissionDenied,
    );
    let backend: Arc<dyn Backend> = fake;
    let out_pair = (remote_dir, backend);
    let err = find_duplicates(true, vec![local_data_dir()], Some(&out_pair)).unwrap_err();
    // 关键：错误不应被改写成 "not a directory"
    assert!(
        !err.to_string().contains("not a directory"),
        "PermissionDenied must propagate, got: {err}"
    );
}

/// `compute_output_prefix` 中 Local 分支 canonicalize 失败 → fallback 到原路径字符串。
/// `full_path` 仅对相对路径调 canonicalize；传一个不存在的相对路径稳定触发 Err。
/// 绝对路径会被 `full_path` 直接透传（不走 canonicalize），不会进 Err arm。
#[test]
fn compute_output_prefix_local_falls_back_when_canonicalize_fails() {
    let out_loc = Location::Local(Utf8PathBuf::from("no_such_relative_dir_xyz_abc"));
    let pair = (out_loc, LocalBackend::arc());
    let prefix = compute_output_prefix(Some(&pair)).expect("Some");
    assert_eq!(prefix, "no_such_relative_dir_xyz_abc");
}

/// `decide_protect` 三 arm 直测让 branch counter 收敛（消 multi-instance phantom miss）：
/// 无 prefix / 有 prefix + `any_under_prefix` / 有 prefix + 无 `any_under_prefix`。
#[test]
fn decide_protect_no_prefix_protects_all() {
    assert_eq!(decide_protect(0, None, "/x", false), (true, false));
    assert_eq!(decide_protect(3, None, "/y", true), (true, false));
}

#[test]
fn decide_protect_prefix_and_any_under_protects_only_under() {
    // any_under_prefix=true 且 path 在 prefix 下 → protect=true, survivor=false
    assert_eq!(
        decide_protect(0, Some("/keep"), "/keep/a", true),
        (true, false)
    );
    // any_under_prefix=true 但 path 在 prefix 外 → protect=false (active remove)
    assert_eq!(
        decide_protect(1, Some("/keep"), "/other/b", true),
        (false, false)
    );
}

#[test]
fn decide_protect_prefix_but_none_under_keeps_first_as_survivor() {
    // any_under_prefix=false 组：首份 idx==0 → survivor + protect
    assert_eq!(
        decide_protect(0, Some("/keep"), "/other/a", false),
        (true, true)
    );
    // 其余 idx>0 → active remove
    assert_eq!(
        decide_protect(1, Some("/keep"), "/other/b", false),
        (false, false)
    );
}

/// `compute_output_prefix` 的 `other => other.display()` arm：
/// 远端 `Location` 走 `Display` 而非 `full_path` canonicalize。`FakeBackend` 模拟
/// SMB 目录让 `is_dir` check 通过、进入 `compute_output_prefix` 后命中 other arm。
#[test]
fn find_duplicates_remote_output_uses_display_for_prefix() {
    use crate::adapters::backend::fake::FakeBackend;

    let fake = Arc::new(FakeBackend::new("smb"));
    let remote_dir = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("out"),
    };
    fake.add_dir(remote_dir.clone());
    let backend: Arc<dyn Backend> = fake;
    let out_pair = (remote_dir, backend);
    find_duplicates(false, vec![local_data_dir()], Some(&out_pair)).unwrap();
}
