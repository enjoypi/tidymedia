// DummyClient::mkdir 直接调用注入 Err 触发 if let Some(k) Err arm。
#[test]
fn dummy_client_mkdir_returns_injected_err() {
    let c = DummyClient {
        mkdir: Some(io::ErrorKind::PermissionDenied),
        ..Default::default()
    };
    let target = DummyTarget::new("/p");
    let err = RemoteClient::<DummyTarget>::mkdir(&c, &target).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

// walk_recursive 的 Dir 分支：list 返 Dir entry → 递归进入子目录。
// 覆盖 `if entry.kind == EntryKind::Dir` True 分支 + `match A::Target::from_location` Ok arm。
#[derive(Debug)]
struct DirListThenEmpty {
    calls: std::sync::atomic::AtomicUsize,
}

impl RemoteClient<DummyTarget> for DirListThenEmpty {
    fn stat(&self, _t: &DummyTarget) -> io::Result<crate::entities::backend::Metadata> {
        unreachable!()
    }
    fn list(&self, _t: &DummyTarget) -> io::Result<Vec<crate::entities::backend::Entry>> {
        use crate::entities::backend::{Entry, EntryKind};
        let n = self
            .calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n == 0 {
            Ok(vec![
                Entry {
                    location: crate::entities::uri::Location::Local(camino::Utf8PathBuf::from(
                        "/dummy/subdir",
                    )),
                    size: 0,
                    kind: EntryKind::Dir,
                },
                Entry {
                    location: crate::entities::uri::Location::Local(camino::Utf8PathBuf::from(
                        "/dummy/file.bin",
                    )),
                    size: 4,
                    kind: EntryKind::File,
                },
            ])
        } else {
            Ok(vec![])
        }
    }
    fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
        unreachable!()
    }
    fn write(&self, _t: &DummyTarget, _data: &[u8]) -> io::Result<u64> {
        unreachable!()
    }
    fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
        unreachable!()
    }
    fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
        unreachable!()
    }
}

#[test]
fn walk_recurses_into_dir_entries() {
    use crate::entities::backend::EntryKind;
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DirListThenEmpty {
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client(client),
    };
    let entries: Vec<_> = b.walk(&loc()).filter_map(Result::ok).collect();
    assert!(entries.iter().any(|e| e.kind == EntryKind::Dir));
    assert!(entries.iter().any(|e| e.kind == EntryKind::File));
}

// walk_recursive 的 visited HashSet 环防护：list 无限返 Dir entry 指向自身
// （from_location 恒返 path="/dummy"）时，无 visited 保护会栈无界增长 → OOM。
// 有 visited 保护：root 已入 visited → 首轮 pop 后 sub target path 命中 visited
// 直接跳 push，栈随即空退出，list 仅被调用一次。
#[test]
fn walk_recursive_breaks_symlink_loop() {
    use crate::entities::backend::EntryKind;

    #[derive(Debug)]
    struct DirListSelfLoop {
        list_calls: std::sync::atomic::AtomicUsize,
    }
    impl RemoteClient<DummyTarget> for DirListSelfLoop {
        fn stat(&self, _t: &DummyTarget) -> io::Result<crate::entities::backend::Metadata> {
            unreachable!()
        }
        fn list(&self, _t: &DummyTarget) -> io::Result<Vec<crate::entities::backend::Entry>> {
            self.list_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // 每次都返一个 Dir entry 指向自身；from_location 恒返 path="/dummy"
            // → 第 2 轮走 visited fast-path 跳过 push，防栈无界。
            Ok(vec![crate::entities::backend::Entry {
                location: crate::entities::uri::Location::Local(camino::Utf8PathBuf::from(
                    "/dummy/self",
                )),
                size: 0,
                kind: EntryKind::Dir,
            }])
        }
        fn read(&self, _t: &DummyTarget) -> io::Result<Vec<u8>> {
            unreachable!()
        }
        fn write(&self, _t: &DummyTarget, _data: &[u8]) -> io::Result<u64> {
            unreachable!()
        }
        fn unlink(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
        fn mkdir(&self, _t: &DummyTarget) -> io::Result<()> {
            unreachable!()
        }
    }
    let client = Arc::new(DirListSelfLoop {
        list_calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client(client.clone() as Arc<dyn RemoteClient<DummyTarget>>),
    };
    let entries: Vec<_> = b.walk(&loc()).collect();
    // 保护生效：list 仅调用一次（root 一层），Dir entry 也应 push 一次。
    assert_eq!(
        client.list_calls.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "visited 保护应让重复 target 不再触发 list（否则无限循环 OOM）"
    );
    let dir_count = entries
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .filter(|e| e.kind == EntryKind::Dir)
        .count();
    assert_eq!(dir_count, 1, "Dir entry 本身仍应 push 一次: {entries:?}");
}

// walk_recursive 中 from_location 对 Dir entry 失败 → 走 line 183 `Err(e) => out.push(Err(e))` arm。
// ctx 设 fail_after(1)：第 1 次 build_target(root) 成功，第 2 次（处理 Dir entry 时）失败。
#[test]
fn walk_recursive_pushes_err_when_subdir_from_location_fails() {
    let client: Arc<dyn RemoteClient<DummyTarget>> = Arc::new(DirListThenEmpty {
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let b = RemoteBackend {
        adapter: DummyAdapter::with_client_and_ctx(client, DummyCtx::fail_after(1)),
    };
    let entries: Vec<_> = b.walk(&loc()).collect();
    let err_count = entries.iter().filter(|r| r.is_err()).count();
    assert!(err_count > 0, "subdir from_location err must surface");
}
