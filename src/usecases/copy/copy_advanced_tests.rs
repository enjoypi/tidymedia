//! `copy` use case 进阶路径测试：`do_copy_propagates_*` / `include_non_media` / `archive_template` / report。
//! 从 `copy_tests.rs` 拆出避免单文件 > 512 行（P0 §6）。

#[cfg(test)]
mod test_advanced {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::super::*;
    use crate::adapters::backend::local::LocalBackend;
    use crate::adapters::report_sink::JsonFileReportSink;
    use crate::entities::backend::Backend;
    use crate::entities::test_common as tc;
    use crate::entities::uri::Location;
    use crate::usecases::report::ReportSink;

    const DEFAULT_TMPL: &str = "{year}/{month}/{valuable_name}";

    fn utf8(p: &Path) -> Utf8PathBuf {
        Utf8PathBuf::from(p.to_str().unwrap())
    }

    fn local_loc(p: &Path) -> Location {
        Location::Local(utf8(p))
    }

    fn local_arc() -> Arc<dyn Backend> {
        LocalBackend::arc()
    }

    fn local_source(p: &Path) -> (Location, Arc<dyn Backend>) {
        (local_loc(p), local_arc())
    }

    fn make_media_info(dir: &Path, name: &str) -> Info {
        let png = tc::copy_png_to(dir, name).unwrap();
        let mut info = Info::from(png.to_str().unwrap()).unwrap();
        info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
        info
    }

    fn default_opts(template: &str) -> CopyOpts<'_> {
        CopyOpts {
            dry_run: false,
            remove: false,
            include_non_media: false,
            template,
        }
    }

    #[test]
    fn do_copy_propagates_exists_error_when_indexed_file_deleted() {
        let dir = tempdir().unwrap();
        let prefix = vec![0u8; 4096];

        let indexed_path = dir.path().join("indexed.bin");
        let mut a = prefix.clone();
        a.push(b'A');
        fs::write(&indexed_path, &a).unwrap();

        let src_path = dir.path().join("source.bin");
        let mut b = prefix;
        b.push(b'B');
        fs::write(&src_path, &b).unwrap();

        let info_src = Info::from(src_path.to_str().unwrap()).unwrap();

        let mut idx = crate::entities::file_index::Index::new();
        idx.insert(indexed_path.to_str().unwrap()).unwrap();
        fs::remove_file(&indexed_path).unwrap();

        let out_dir = local_loc(dir.path());
        let err = do_copy(
            &info_src,
            &out_dir,
            &local_arc(),
            &mut idx,
            &default_opts(DEFAULT_TMPL),
        )
        .unwrap_err();
        let _ = err;
    }

    // remove=true + dry_run=false + dup 存在 + 源文件父目录 read-only → remove 失败。
    #[test]
    #[cfg(unix)]
    fn do_copy_remove_source_propagates_error() {
        use std::os::unix::fs::PermissionsExt;
        let src_dir = tempdir().unwrap();
        let src_parent = src_dir.path().join("locked");
        fs::create_dir(&src_parent).unwrap();
        let png_path = tc::copy_png_to(&src_parent, "photo.png").unwrap();
        let info = Info::from(png_path.to_str().unwrap()).unwrap();

        let mut idx = crate::entities::file_index::Index::new();
        idx.insert(png_path.to_str().unwrap()).unwrap();

        let mut perms = fs::metadata(&src_parent).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o555);
        fs::set_permissions(&src_parent, perms.clone()).unwrap();

        let out_dir = local_loc(src_dir.path());
        let opts = CopyOpts {
            dry_run: false,
            remove: true,
            include_non_media: false,
            template: DEFAULT_TMPL,
        };
        let res = do_copy(&info, &out_dir, &local_arc(), &mut idx, &opts);

        perms.set_mode(original_mode);
        fs::set_permissions(&src_parent, perms).unwrap();

        assert!(res.is_err(), "expected remove failure but got {res:?}");
    }

    // 在 target 的预期子目录路径上放一个**文件**，让 create_all 失败。
    #[test]
    fn do_copy_create_dir_all_fails_when_path_blocked_by_file() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        fs::write(out.path().join("2024"), b"i am a file").unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let err = do_copy(
            &info,
            &local_loc(out.path()),
            &local_arc(),
            &mut idx,
            &default_opts(DEFAULT_TMPL),
        )
        .unwrap_err();
        let _ = err;
    }

    // 源文件 chmod 000 + remove=false → copy 失败。
    #[test]
    #[cfg(unix)]
    fn do_copy_file_copy_fails_when_source_unreadable() {
        use std::os::unix::fs::PermissionsExt;
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");

        let mut perms = fs::metadata(info.full_path.as_str()).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o000);
        fs::set_permissions(info.full_path.as_str(), perms.clone()).unwrap();

        let out = tempdir().unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let res = do_copy(
            &info,
            &local_loc(out.path()),
            &local_arc(),
            &mut idx,
            &default_opts(DEFAULT_TMPL),
        );

        perms.set_mode(original_mode);
        fs::set_permissions(info.full_path.as_str(), perms).unwrap();

        assert!(res.is_err(), "expected copy failure but got {res:?}");
    }

    // local→local + remove=true 命中 fast-path 走 fs::rename：src 父目录 chmod 555 后
    // fs::rename 失去 dentry write 权限直接失败，Err 传播。覆盖 fast-path if-branch 的
    // rename Err 路径（替代原"stream_copy 后 remove fail"语义——fast-path 下 rename 是
    // 单步原子操作，没有"copy 成功但 remove 失败"中间态）。
    #[test]
    #[cfg(unix)]
    fn do_copy_move_fails_when_src_parent_locked() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempdir().unwrap();
        let src_parent = root.path().join("locked_src");
        fs::create_dir(&src_parent).unwrap();
        let info = make_media_info(&src_parent, "photo.png");

        let out = root.path().join("out");
        fs::create_dir(&out).unwrap();
        let mut idx = crate::entities::file_index::Index::new();

        let mut perms = fs::metadata(&src_parent).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o555);
        fs::set_permissions(&src_parent, perms.clone()).unwrap();

        let opts = CopyOpts {
            dry_run: false,
            remove: true,
            include_non_media: false,
            template: DEFAULT_TMPL,
        };
        let res = do_copy(&info, &local_loc(&out), &local_arc(), &mut idx, &opts);

        perms.set_mode(original_mode);
        fs::set_permissions(&src_parent, perms).unwrap();

        assert!(res.is_err(), "expected rename failure but got {res:?}");
    }

    fn walk_files(root: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(p) = stack.pop() {
            if let Ok(entries) = fs::read_dir(&p) {
                for e in entries.flatten() {
                    let path = e.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        out.push(path);
                    }
                }
            }
        }
        out
    }

    // include_non_media=true → 非媒体（.txt 等 magic-bytes MIME 未识别为 image/video 的）也被搬运
    #[test]
    fn copy_include_non_media_copies_plain_files() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("readme.txt"), b"hello world").unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            false,
            true,
            None,
            None,
        )
        .unwrap();
        let copied = walk_files(out.path());
        assert!(
            !copied.is_empty(),
            "include_non_media must copy non-media files"
        );
    }

    // do_copy 级别覆盖 include_non_media=true 写出路径
    #[test]
    fn do_copy_include_non_media_writes_non_media() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("doc.txt"), b"plain document").unwrap();
        let out = tempdir().unwrap();
        let info = Info::from(src.path().join("doc.txt").to_str().unwrap()).unwrap();
        assert!(!info.is_media());
        let mut idx = crate::entities::file_index::Index::new();
        let opts = CopyOpts {
            dry_run: false,
            remove: false,
            include_non_media: true,
            template: DEFAULT_TMPL,
        };
        let did = do_copy(&info, &local_loc(out.path()), &local_arc(), &mut idx, &opts).unwrap();
        assert!(did, "non-media must be copied when include_non_media=true");
    }

    // include_non_media=false（默认）+ 非媒体 → 跳过，行为对照
    #[test]
    fn do_copy_default_skips_non_media() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("doc.txt"), b"plain document").unwrap();
        let out = tempdir().unwrap();
        let info = Info::from(src.path().join("doc.txt").to_str().unwrap()).unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let did = do_copy(
            &info,
            &local_loc(out.path()),
            &local_arc(),
            &mut idx,
            &default_opts(DEFAULT_TMPL),
        )
        .unwrap();
        assert!(!did);
        assert_eq!(walk_files(out.path()).len(), 0);
    }

    // generate_unique_name 的 empty-render 路径：template 只含 {valuable_name} 且路径无非 ASCII
    // → sub_dir_rel 为空串 → 直接用 output_dir 作 sub_dir_path（L343 True 分支）。
    #[test]
    fn generate_unique_name_empty_template_result_uses_output_dir() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        // 路径无非 ASCII → valuable_name 为空 → "{valuable_name}" 渲染为 ""
        let out = tempdir().unwrap();
        let (_, target) = generate_unique_name(
            &info,
            &local_loc(out.path()),
            &local_arc(),
            "{valuable_name}",
        )
        .unwrap()
        .expect("should generate even with empty subdir");
        // 目标直接在 output_dir 下（无子目录层）
        assert!(
            target.display().ends_with("photo.png"),
            "got: {}",
            target.display()
        );
    }

    #[test]
    fn copy_with_custom_archive_template_uses_day() {
        let src = tempdir().unwrap();
        let nested = src.path().join("test_album");
        fs::create_dir_all(&nested).unwrap();
        tc::copy_png_to(&nested, "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            false,
            false,
            Some("{year}/{month}/{day}"),
            None,
        )
        .unwrap();
        // mtime 固定为 2024-01-01，所以 day = "01"
        let expected = out
            .path()
            .join("2024")
            .join("01")
            .join("01")
            .join("photo.png");
        assert!(expected.exists(), "expected file at {expected:?}");
    }

    #[test]
    fn copy_with_report_path_creates_valid_json() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        let report_dir = tempdir().unwrap();
        let report_path = report_dir.path().join("report.json");
        let sink = JsonFileReportSink::new(report_path.to_str().unwrap());
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            true,
            false,
            false,
            None,
            Some(&sink as &dyn ReportSink),
        )
        .unwrap();
        let content = fs::read_to_string(&report_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["dry_run"], true);
        assert!(parsed["scanned"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn copy_empty_source_with_report_writes_zero_counts() {
        let src = tempdir().unwrap();
        let out = tempdir().unwrap();
        let report_dir = tempdir().unwrap();
        let report_path = report_dir.path().join("empty_report.json");
        let sink = JsonFileReportSink::new(report_path.to_str().unwrap());
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            false,
            false,
            None,
            Some(&sink as &dyn ReportSink),
        )
        .unwrap();
        let content = fs::read_to_string(&report_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["scanned"], 0);
        assert_eq!(parsed["copied"], 0);
    }
}
