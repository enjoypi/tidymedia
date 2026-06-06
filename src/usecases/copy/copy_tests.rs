#[cfg(test)]
mod test {
    use super::super::*;

    /// 结构化日志 summary 的 result 维度：0 失败 → "ok"，>0 → "partial"。
    /// 直测 helper —— 该值只进 tracing 字段，集成测试不捕日志杀不掉 `==` 变异。
    #[test]
    fn summary_result_maps_failed_count_to_dimension() {
        assert_eq!(summary_result(0), "ok");
        assert_eq!(summary_result(3), "partial");
    }

    #[test]
    fn test_match_non_english() {
        assert!(!any_non_english("abc"));
        assert!(any_non_english("abc中文"));
        // 边界值 0x7F（DEL）仍是 ASCII：杀 `> 127` 被变异成 `>= 127`
        assert!(!any_non_english("\u{7f}"));
    }

    #[test]
    fn extract_valuable_name_finds_last_non_english_dir() {
        let path = Utf8Path::new("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/中文/abc");
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

    /// 单段非英文路径：len==1 不 pop，整段就是 valuable name。
    /// 杀 `components.len() > 1` 被变异成 `>= 1`（单段也被弹掉 → 误返空）。
    #[test]
    fn extract_valuable_name_keeps_single_non_english_component() {
        let path = Utf8Path::new("中文相册");
        assert_eq!(extract_valuable_name(path), "中文相册");
    }

    /// 非英文出现在最后一段（文件名）：必须被 pop 排除 → 返回空。
    /// 杀 `components.len() > 1` 被变异成 `== 1`（多段路径不再弹文件名）。
    #[test]
    fn extract_valuable_name_excludes_filename_component() {
        let path = Utf8Path::new("/a/b/中文名.jpg");
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

    #[test]
    fn chrono_offset_from_hours_valid() {
        let off = chrono_offset_from_hours(8);
        assert_eq!(off.local_minus_utc(), 8 * 3600);
    }

    #[test]
    fn chrono_offset_from_hours_out_of_range_falls_back_to_utc() {
        // 25*3600 秒超过 chrono FixedOffset 的 ±24h 边界，应回退 UTC
        let off = chrono_offset_from_hours(25);
        assert_eq!(off.local_minus_utc(), 0);
    }
}

#[cfg(test)]
mod test_io {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::super::*;
    use crate::adapters::backend::local::LocalBackend;
    use crate::entities::backend::Backend;
    use crate::entities::test_common as tc;
    use crate::entities::uri::Location;

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

    #[test]
    fn copy_empty_source_returns_ok() {
        let src = tempdir().unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn copy_dry_run_does_not_write() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            true,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn copy_writes_into_year_month_valuable_path() {
        let src = tempdir().unwrap();
        let nested = src.path().join("假日相册");
        fs::create_dir_all(&nested).unwrap();
        tc::copy_png_to(&nested, "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            false,
            false,
            None,
            None,
        )
        .unwrap();
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
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
    }

    #[test]
    fn move_removes_source_when_duplicate_exists() {
        let src = tempdir().unwrap();
        let png_src = tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        fs::copy(tc::DATA_DNS_BENCHMARK, out.path().join("already.png")).unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            true,
            false,
            None,
            None,
        )
        .unwrap();
        assert!(!png_src.exists(), "source duplicate should be removed");
    }

    #[test]
    fn move_renames_into_output() {
        let root = tempdir().unwrap();
        let src_dir = root.path().join("src");
        let out_dir = root.path().join("out");
        fs::create_dir_all(&src_dir).unwrap();
        let png_src = tc::copy_png_to(&src_dir, "photo.png").unwrap();
        copy(
            &[local_source(&src_dir)],
            local_source(&out_dir),
            false,
            true,
            false,
            None,
            None,
        )
        .unwrap();
        assert!(!png_src.exists());
        let expected = out_dir.join("2024").join("01").join("photo.png");
        assert!(expected.exists(), "expected moved file at {expected:?}");
    }

    #[test]
    fn do_copy_skips_non_media_files() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("plain.bin"), b"abc").unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            false,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }
}
