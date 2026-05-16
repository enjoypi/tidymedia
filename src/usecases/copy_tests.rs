#[cfg(test)]
mod test {
    use super::super::*;

    #[test]
    fn test_match_non_english() {
        assert!(!any_non_english("abc"));
        assert!(any_non_english("abc中文"));
    }

    #[test]
    fn extract_valuable_name_finds_last_non_english_dir() {
        let path = Utf8Path::new(
            "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/中文/abc",
        );
        assert_eq!(extract_valuable_name(path), "中文");
    }

    #[test]
    fn extract_valuable_name_returns_empty_when_all_ascii() {
        let path = Utf8Path::new("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z");
        assert_eq!(extract_valuable_name(path), "");
    }

    #[test]
    fn extract_valuable_name_root_returns_empty() {
        let path = Utf8Path::new("/");
        assert_eq!(extract_valuable_name(path), "");
    }

    #[test]
    fn extract_valuable_name_single_component_returns_empty() {
        let path = Utf8Path::new("photo");
        assert_eq!(extract_valuable_name(path), "");
    }

    #[test]
    fn extract_valuable_name_picks_innermost_non_english() {
        let path = Utf8Path::new("/外层/内层/file.png");
        assert_eq!(extract_valuable_name(path), "内层");
    }

    #[test]
    fn extract_valuable_name_handles_mixed_chars() {
        let path = Utf8Path::new("/p/a高一 元旦晚会/sub/p.png");
        assert_eq!(extract_valuable_name(path), "a高一 元旦晚会");
    }

    #[test]
    fn offset_from_hours_valid_value_produces_expected_offset() {
        let off = offset_from_hours(8);
        assert_eq!(off.whole_hours(), 8);
    }

    #[test]
    fn offset_from_hours_out_of_range_falls_back_to_utc() {
        // 127*3600 秒远超 time crate ±25:59:59 范围，应回退 UTC
        let off = offset_from_hours(127);
        assert_eq!(off, UtcOffset::UTC);
    }
}

#[cfg(test)]
mod test_io {
    use std::fs;
    use std::path::Path;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use crate::entities::test_common as tc;
    use super::super::*;

    fn utf8(p: &Path) -> Utf8PathBuf {
        Utf8PathBuf::from(p.to_str().unwrap())
    }

    fn make_media_info(dir: &Path, name: &str) -> Info {
        let png = tc::copy_png_to(dir, name).unwrap();
        let mut info = Info::from(png.to_str().unwrap()).unwrap();
        info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
        info
    }

    fn fill_collisions(sub: &Path) {
        fs::create_dir_all(sub).unwrap();
        fs::write(sub.join("photo.png"), b"").unwrap();
        for i in 1..10 {
            fs::write(sub.join(format!("photo_{i}.png")), b"").unwrap();
        }
    }

    #[test]
    fn copy_empty_source_returns_ok() {
        let src = tempdir().unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn copy_dry_run_does_not_write() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), true, false, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn copy_writes_into_year_month_valuable_path() {
        let src = tempdir().unwrap();
        let nested = src.path().join("假日相册");
        fs::create_dir_all(&nested).unwrap();
        tc::copy_png_to(&nested, "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false, false).unwrap();
        let expected = out
            .path()
            .join("2024")
            .join("01")
            .join("假日相册")
            .join("photo.png");
        assert!(expected.exists(), "expected file at {expected:?}");
    }

    #[test]
    fn copy_skips_duplicate_already_in_output() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        fs::copy(tc::DATA_DNS_BENCHMARK, out.path().join("already.png")).unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
    }

    #[test]
    fn move_removes_source_when_duplicate_exists() {
        let src = tempdir().unwrap();
        let png_src = tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        fs::copy(tc::DATA_DNS_BENCHMARK, out.path().join("already.png")).unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, true, false).unwrap();
        assert!(!png_src.exists(), "source duplicate should be removed");
    }

    #[test]
    fn move_renames_into_output() {
        let root = tempdir().unwrap();
        let src_dir = root.path().join("src");
        let out_dir = root.path().join("out");
        fs::create_dir_all(&src_dir).unwrap();
        let png_src = tc::copy_png_to(&src_dir, "photo.png").unwrap();
        copy(vec![utf8(&src_dir)], utf8(&out_dir), false, true, false).unwrap();
        assert!(!png_src.exists());
        let expected = out_dir.join("2024").join("01").join("photo.png");
        assert!(expected.exists(), "expected moved file at {expected:?}");
    }

    #[test]
    fn do_copy_skips_non_media_files() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("plain.bin"), b"abc").unwrap();
        let out = tempdir().unwrap();
        copy(vec![utf8(src.path())], utf8(out.path()), false, false, false).unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn generate_unique_name_uses_suffix_when_first_taken() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        let out_utf8 = utf8(out.path());
        let sub = out.path().join("2024").join("01");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("photo.png"), b"x").unwrap();
        let (_, target) = generate_unique_name(&info, &out_utf8)
            .expect("unique name should be generated");
        assert!(target.ends_with("photo_1.png"), "got {target}");
    }

    #[test]
    fn generate_unique_name_none_after_10_collisions() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        fill_collisions(&out.path().join("2024").join("01"));
        let res = generate_unique_name(&info, &utf8(out.path()));
        assert!(res.is_none(), "should exhaust after 10 collisions");
    }

    #[test]
    fn do_copy_errors_when_unique_name_exhausted() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        fill_collisions(&out.path().join("2024").join("01"));
        let mut idx = crate::entities::file_index::Index::new();
        let err = do_copy(&info, &utf8(out.path()), &mut idx, false, false, false)
            .expect_err("must error after collisions");
        assert!(err.to_string().contains("无法为"));
    }

    #[test]
    fn copy_logs_failure_when_target_collisions_exhausted() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        fill_collisions(&out.path().join("2024").join("01"));
        copy(vec![utf8(src.path())], utf8(out.path()), false, false, false).unwrap();
    }

    #[test]
    fn do_copy_dry_run_reports_target_but_writes_nothing() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let did_copy = do_copy(&info, &utf8(out.path()), &mut idx, true, false, false).unwrap();
        assert!(did_copy);
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    // 启用 trace 级别 subscriber，让 copy() 里的 trace! 宏闭包被求值，覆盖 L62 region。
    #[test]
    fn copy_with_trace_subscriber_executes_trace_branch() {
        use tracing_subscriber::EnvFilter;
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("trace"))
            .with_writer(std::io::sink)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let src = tempdir().unwrap();
            tc::copy_png_to(src.path(), "photo.png").unwrap();
            let out = tempdir().unwrap();
            copy(vec![utf8(src.path())], utf8(out.path()), true, false, false).unwrap();
        });
    }

    // output 是一个不存在的相对路径 + dry_run（跳过 mkdir），让 full_path canonicalize 失败 → L66。
    #[test]
    fn copy_with_nonexistent_relative_output_errors() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let bogus_output = Utf8PathBuf::from("definitely-does-not-exist-zzz-relative-xyz");
        let err = copy(vec![utf8(src.path())], bogus_output, true, false, false).unwrap_err();
        let _ = err;
    }

    // output 是已存在文件（非目录），fs_extra::dir::create_all 失败 → L68。
    #[test]
    fn copy_with_output_as_file_errors() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out_file = tempfile::NamedTempFile::new().unwrap();
        let out_path = Utf8PathBuf::from(out_file.path().to_str().unwrap());
        let err = copy(vec![utf8(src.path())], out_path, false, false, false).unwrap_err();
        let _ = err;
    }

    // output_index 中保存的 Info 指向的文件被外部删除 → exists() 失败传播 → L125。
    #[test]
    fn do_copy_propagates_exists_error_when_indexed_file_deleted() {
        let dir = tempdir().unwrap();
        let prefix = vec![0u8; 4096];

        let indexed_path = dir.path().join("indexed.bin");
        let mut a = prefix.clone();
        a.push(b'A');
        fs::write(&indexed_path, &a).unwrap();

        let src_path = dir.path().join("source.bin");
        let mut b = prefix.clone();
        b.push(b'B');
        fs::write(&src_path, &b).unwrap();

        let info_src = Info::from(src_path.to_str().unwrap()).unwrap();

        let mut idx = crate::entities::file_index::Index::new();
        idx.insert(indexed_path.to_str().unwrap()).unwrap();
        fs::remove_file(&indexed_path).unwrap();

        let out_dir = utf8(dir.path());
        let err = do_copy(&info_src, &out_dir, &mut idx, false, false, false).unwrap_err();
        let _ = err;
    }

    // remove=true + dry_run=false + dup 存在 + 源文件父目录 read-only → remove 失败 → L135。
    #[test]
    #[cfg(unix)]
    fn do_copy_remove_source_propagates_error() {
        use std::os::unix::fs::PermissionsExt;
        let src_dir = tempdir().unwrap();
        let src_parent = src_dir.path().join("locked");
        fs::create_dir(&src_parent).unwrap();
        let png_path = tc::copy_png_to(&src_parent, "photo.png").unwrap();
        let info = Info::from(png_path.to_str().unwrap()).unwrap();

        // dup 存在于 output_index：把 source 自己 insert 进 idx，这样 exists 返回 Some
        let mut idx = crate::entities::file_index::Index::new();
        idx.insert(png_path.to_str().unwrap()).unwrap();

        // 把 src 父目录设为只读，让 fs_extra::file::remove 失败
        let mut perms = fs::metadata(&src_parent).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o555);
        fs::set_permissions(&src_parent, perms.clone()).unwrap();

        let out_dir = utf8(src_dir.path());
        let res = do_copy(&info, &out_dir, &mut idx, false, true, false);

        // 恢复权限便于 tempdir 清理
        perms.set_mode(original_mode);
        fs::set_permissions(&src_parent, perms).unwrap();

        assert!(res.is_err(), "expected remove failure but got {res:?}");
    }

    // 在 target 的预期子目录路径上放一个**文件**，让 fs_extra::dir::create_all 失败 → L157。
    #[test]
    fn do_copy_create_dir_all_fails_when_path_blocked_by_file() {
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");
        let out = tempdir().unwrap();
        // 创建 2024 作为**文件**，让后续 create_all("2024/01/...") 失败
        fs::write(out.path().join("2024"), b"i am a file").unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let err = do_copy(&info, &utf8(out.path()), &mut idx, false, false, false).unwrap_err();
        let _ = err;
    }

    // 源文件 chmod 000 + remove=false → fs_extra::file::copy 失败 → L164。
    #[test]
    #[cfg(unix)]
    fn do_copy_file_copy_fails_when_source_unreadable() {
        use std::os::unix::fs::PermissionsExt;
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");

        // chmod 000 让 fs_extra::file::copy 内部 open 失败
        let mut perms = fs::metadata(&info.full_path.as_str()).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o000);
        fs::set_permissions(info.full_path.as_str(), perms.clone()).unwrap();

        let out = tempdir().unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let res = do_copy(&info, &utf8(out.path()), &mut idx, false, false, false);

        perms.set_mode(original_mode);
        fs::set_permissions(info.full_path.as_str(), perms).unwrap();

        assert!(res.is_err(), "expected copy failure but got {res:?}");
    }

    // 同上，但 remove=true 走 move_file 路径 → L162。
    #[test]
    #[cfg(unix)]
    fn do_copy_file_move_fails_when_source_unreadable() {
        use std::os::unix::fs::PermissionsExt;
        let src = tempdir().unwrap();
        let info = make_media_info(src.path(), "photo.png");

        let mut perms = fs::metadata(&info.full_path.as_str()).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o000);
        fs::set_permissions(info.full_path.as_str(), perms.clone()).unwrap();

        let out = tempdir().unwrap();
        let mut idx = crate::entities::file_index::Index::new();
        let res = do_copy(&info, &utf8(out.path()), &mut idx, false, true, false);

        perms.set_mode(original_mode);
        fs::set_permissions(info.full_path.as_str(), perms).unwrap();

        assert!(res.is_err(), "expected move failure but got {res:?}");
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
        copy(vec![utf8(src.path())], utf8(out.path()), false, false, true).unwrap();
        let copied = walk_files(out.path());
        assert!(!copied.is_empty(), "include_non_media must copy non-media files");
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
        let did = do_copy(&info, &utf8(out.path()), &mut idx, false, false, true).unwrap();
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
        let did = do_copy(&info, &utf8(out.path()), &mut idx, false, false, false).unwrap();
        assert!(!did);
        assert_eq!(walk_files(out.path()).len(), 0);
    }
}
