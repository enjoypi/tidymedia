#[test]
fn move_text_shot_same_remote_scheme_uses_stream_copy_not_rename_fast_path() {
    // 顶置局部类型：items_after_statements (pedantic)。
    struct PerSchemeFactory {
        src: Arc<dyn Backend>,
        out: Arc<dyn Backend>,
        out_path: Utf8PathBuf,
    }
    impl BackendFactory for PerSchemeFactory {
        fn for_location(&self, loc: &Location) -> common::Result<Arc<dyn Backend>> {
            // 同 scheme（smb），但内容用 path 区分：output 走 self.out
            Ok(if loc.path() == self.out_path {
                Arc::clone(&self.out)
            } else {
                Arc::clone(&self.src)
            })
        }
    }

    // 两 backend 同 scheme 但非 "local" → do_move_file 第二个 && 短路 False 进入
    // stream_copy 路径，覆盖 BR idx 1。
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("smb"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc, tiny_png());

    let out_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/dst"),
    };
    let factory = PerSchemeFactory {
        src: Arc::clone(&src_fake) as Arc<dyn Backend>,
        out: Arc::clone(&out_fake) as Arc<dyn Backend>,
        out_path: out_loc.path().to_path_buf(),
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &out_loc,
        false,
    )
    .unwrap();
    assert_eq!(report.moved, 1);
}

#[test]
fn move_text_shot_cross_scheme_stream_copy_writer_runtime_error() {
    // open_write 成功但 writer.write 立即报错 → std::io::copy Err → stream_copy
    // Err arm（DA:324-325, BRDA:322 idx 0）+ 清理半截目标文件。
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc.clone(), tiny_png());
    out_fake.inject_writer_error(local_loc("/out/img.png"), io::ErrorKind::PermissionDenied);

    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    // src 仍在；dst 被半截清理（remove_file 调用了，但 FakeBackend 没 dst 入 files 故 noop）
    assert!(src_fake.exists(&src_loc).unwrap());
}

#[test]
fn move_text_shot_cross_scheme_stream_copy_open_write_fails() {
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc.clone(), tiny_png());
    out_fake.inject_error(
        local_loc("/out/img.png"),
        crate::FakeOp::OpenWrite,
        io::ErrorKind::PermissionDenied,
    );

    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    // src 未删（stream_copy Err 前）
    assert!(src_fake.exists(&src_loc).unwrap());
}

#[test]
fn move_text_shot_cross_scheme_remove_src_failure_marks_half_state() {
    let src_fake = Arc::new(FakeBackend::new("smb"));
    let out_fake = Arc::new(FakeBackend::new("local"));
    let src_loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "p".into(),
        path: Utf8PathBuf::from("/img.png"),
    };
    src_fake.add_file(src_loc.clone(), tiny_png());
    src_fake.inject_error(
        src_loc.clone(),
        crate::FakeOp::RemoveFile,
        io::ErrorKind::PermissionDenied,
    );

    let factory = TwoSchemeFactory {
        smb: Arc::clone(&src_fake) as Arc<dyn Backend>,
        local: Arc::clone(&out_fake) as Arc<dyn Backend>,
    };
    let detector = FakeTextDetector::new(true);
    let report = move_text_shot(
        &detector,
        &factory,
        &[Location::Smb {
            user: None,
            host: "nas".into(),
            port: None,
            share: "p".into(),
            path: Utf8PathBuf::from("/"),
        }],
        &local_loc("/out"),
        false,
    )
    .unwrap();
    assert_eq!(report.failed, 1);
    // dst 已写入；src 未删（半态错误）
    assert!(out_fake.exists(&local_loc("/out/img.png")).unwrap());
    assert!(src_fake.exists(&src_loc).unwrap());
    let msg = &report.errors[0].message;
    assert!(
        msg.contains("copied") && msg.contains("but cannot remove source"),
        "got: {msg}"
    );
}

#[test]
fn move_text_shot_propagates_factory_error_for_output() {
    // factory.for_location(output)? Err arm（line 43）
    struct OutputFails;
    impl BackendFactory for OutputFails {
        fn for_location(&self, _loc: &Location) -> common::Result<Arc<dyn Backend>> {
            Err(crate::entities::common::Error::Io(io::Error::new(
                io::ErrorKind::Unsupported,
                "no factory",
            )))
        }
    }
    let detector = FakeTextDetector::new(true);
    let err = move_text_shot(
        &detector,
        &OutputFails,
        &[local_loc("/src")],
        &local_loc("/out"),
        false,
    )
    .unwrap_err();
    let Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn move_text_shot_propagates_factory_error_for_source() {
    // factory.for_location(source)? Err arm（line 58）：output 成功，source 失败
    struct SourceFails {
        ok_for: Utf8PathBuf,
    }
    impl BackendFactory for SourceFails {
        fn for_location(&self, loc: &Location) -> common::Result<Arc<dyn Backend>> {
            if loc.path() == self.ok_for {
                Ok(Arc::new(FakeBackend::new("local")) as Arc<dyn Backend>)
            } else {
                Err(crate::entities::common::Error::Io(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "src failed",
                )))
            }
        }
    }
    let factory = SourceFails {
        ok_for: Utf8PathBuf::from("/out"),
    };
    let detector = FakeTextDetector::new(true);
    let err = move_text_shot(
        &detector,
        &factory,
        &[local_loc("/src")],
        &local_loc("/out"),
        true,
    )
    .unwrap_err();
    let Error::Io(io_err) = err;
    assert_eq!(io_err.kind(), io::ErrorKind::Unsupported);
}
