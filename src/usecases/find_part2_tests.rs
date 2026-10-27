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
    let err = find_duplicates(true, vec![local_data_dir()], Some(&out_pair), &mut Vec::new()).unwrap_err();
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

#[test]
fn find_duplicates_remote_output_uses_storage_key_prefix() {
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
    find_duplicates(false, vec![local_data_dir()], Some(&out_pair), &mut Vec::new()).unwrap();
}

#[test]
fn render_script_relative_paths_protect_archived_copy_under_canonical_prefix() {
    let dir = tempdir().unwrap();
    let photos = dir.path().join("photos");
    let archived = photos.join("archive/2024");
    std::fs::create_dir_all(&archived).unwrap();
    std::fs::write(archived.join("img.jpg"), b"dup").unwrap();
    let dupdir = photos.join("0_dup");
    std::fs::create_dir_all(&dupdir).unwrap();
    std::fs::write(dupdir.join("img.jpg"), b"dup").unwrap();

    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let out_pair = (
        Location::Local(Utf8PathBuf::from("photos/archive")),
        LocalBackend::arc(),
    );
    let prefix = compute_output_prefix(Some(&out_pair)).expect("Some");

    let groups = vec![DuplicateGroup {
        size: 3,
        paths: vec![
            Utf8PathBuf::from("photos/0_dup/img.jpg"),
            Utf8PathBuf::from("photos/archive/2024/img.jpg"),
        ],
    }];
    let out = run_render(&groups, Some(&prefix));
    std::env::set_current_dir(original).unwrap();
    assert!(
        out.contains("# os.remove(\"photos/archive/2024/img.jpg\")\n"),
        "archived copy must stay commented under canonical prefix: {out}"
    );
    assert!(
        !out.contains("# SURVIVOR"),
        "must not fall back to survivor heuristic when prefix matches: {out}"
    );
    assert!(out.contains("os.remove(\"photos/0_dup/img.jpg\")\n"));
    assert!(!out.contains("# os.remove(\"photos/0_dup/img.jpg\")"));
}

#[cfg(unix)]
#[test]
fn render_script_symlinked_source_path_matches_canonical_output_prefix() {
    let dir = tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir_all(real.join("2024")).unwrap();
    std::fs::write(real.join("2024/img.jpg"), b"dup").unwrap();
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("img.jpg"), b"dup").unwrap();

    let prefix = crate::entities::common::key_from_display(real.to_str().unwrap());
    let groups = vec![DuplicateGroup {
        size: 3,
        paths: vec![
            Utf8PathBuf::from(other.join("img.jpg").to_str().unwrap()),
            Utf8PathBuf::from(alias.join("2024/img.jpg").to_str().unwrap()),
        ],
    }];
    let out = run_render(&groups, Some(&prefix));
    let alias_lit = alias.join("2024/img.jpg").to_str().unwrap().to_string();
    assert!(
        out.contains(&format!("# os.remove(\"{alias_lit}\")\n")),
        "archived copy via symlink must stay commented: {out}"
    );
    assert!(
        !out.contains("# SURVIVOR"),
        "must not fall back to survivor heuristic when canonical prefix matches: {out}"
    );
    let other_lit = other.join("img.jpg").to_str().unwrap().to_string();
    assert!(out.contains(&format!("os.remove(\"{other_lit}\")\n")));
    assert!(!out.contains(&format!("# os.remove(\"{other_lit}\")")));
}

#[cfg(unix)]
#[test]
fn compute_output_prefix_resolves_symlink_components() {
    let dir = tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let pair = (
        Location::Local(Utf8PathBuf::from(alias.to_str().unwrap())),
        LocalBackend::arc(),
    );
    let prefix = compute_output_prefix(Some(&pair)).expect("Some");
    assert_eq!(
        prefix,
        crate::entities::common::key_from_display(real.to_str().unwrap())
    );
}

#[test]
fn render_script_propagates_write_failure_at_every_stage() {
    struct FailAfter {
        budget: usize,
    }

    impl std::io::Write for FailAfter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.budget == 0 {
                return Err(std::io::Error::other("injected stdout failure"));
            }
            self.budget -= 1;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn write_fmt(&mut self, _: std::fmt::Arguments<'_>) -> std::io::Result<()> {
            if self.budget == 0 {
                return Err(std::io::Error::other("injected stdout failure"));
            }
            self.budget -= 1;
            Ok(())
        }
    }

    let dir = tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir_all(real.join("2024")).unwrap();
    std::fs::write(real.join("2024/img.jpg"), b"dup").unwrap();
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("img.jpg"), b"dup").unwrap();
    let keyed = vec![DuplicateGroup {
        size: 3,
        paths: vec![
            Utf8PathBuf::from(real.join("2024/img.jpg").to_str().unwrap()),
            Utf8PathBuf::from(other.join("img.jpg").to_str().unwrap()),
        ],
    }];
    let real_prefix = crate::entities::common::key_from_display(real.to_str().unwrap());

    for budget in 0..9 {
        let mut sink = FailAfter { budget };
        let err = render_script(&sample_two_groups(), None, &mut sink).unwrap_err();
        assert!(
            err.to_string().contains("injected stdout failure"),
            "budget {budget} must fail"
        );
    }
    for budget in 0..11 {
        let mut sink = FailAfter { budget };
        let err =
            render_script(&sample_two_groups(), Some("/no_such_output_xyz"), &mut sink)
                .unwrap_err();
        assert!(
            err.to_string().contains("injected stdout failure"),
            "budget {budget} must fail"
        );
    }
    for budget in 0..5 {
        let mut sink = FailAfter { budget };
        let err = render_script(&keyed, Some(&real_prefix), &mut sink).unwrap_err();
        assert!(
            err.to_string().contains("injected stdout failure"),
            "budget {budget} must fail"
        );
    }
}

#[test]
fn find_scanned_counts_indexed_empty_and_walker_errors() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.bin"), b"dup").unwrap();
    std::fs::write(dir.path().join("b.bin"), b"dup").unwrap();
    std::fs::write(dir.path().join("empty.bin"), b"").unwrap();
    let missing = (
        Location::Local(Utf8PathBuf::from("/no/such/dir/zzz_find_scanned")),
        LocalBackend::arc(),
    );
    let report = find_duplicates(
        true,
        vec![local_dir(dir.path()), missing],
        None,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(report.scanned, 4, "indexed 2 + empty 1 + walker_errors 1");
}

#[test]
fn find_duplicates_propagates_script_sink_failure() {
    struct FailSink;

    impl std::io::Write for FailSink {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected script sink failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.bin"), b"dup").unwrap();
    std::fs::write(dir.path().join("b.bin"), b"dup").unwrap();
    let err = find_duplicates(true, vec![local_dir(dir.path())], None, &mut FailSink)
        .unwrap_err();
    assert!(
        err.to_string().contains("injected script sink failure"),
        "got: {err}"
    );
}
