//! source/output 重叠保护测试：source ⊆ output 拒绝 + output ⊂ source 就地归档排除。

use std::fs;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::*;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;

fn local_source(p: &Path) -> (Location, Arc<dyn Backend>) {
    (
        Location::Local(Utf8PathBuf::from(p.to_str().unwrap())),
        LocalBackend::arc(),
    )
}

fn run(
    src: &Path,
    out: &Path,
    remove: bool,
) -> crate::entities::common::Result<crate::usecases::report::CopyReport> {
    copy(
        &[local_source(src)],
        local_source(out),
        /* dry_run = */ false,
        remove,
        /* include_non_media = */ false,
        None,
        None,
    )
}

// source == output：move 会把每个源文件判为 output 中已存在的副本并删除自身。
#[test]
fn copy_rejects_source_equal_to_output() {
    let dir = tempdir().unwrap();
    tc::copy_png_to(dir.path(), "a.png").unwrap();
    let err = run(dir.path(), dir.path(), false).unwrap_err();
    assert!(err.to_string().contains("inside output"), "got: {err}");
}

// source 位于 output 子树内：同上，必须 fail fast。
#[test]
fn copy_rejects_source_inside_output() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    tc::copy_png_to(&sub, "a.png").unwrap();
    let err = run(&sub, dir.path(), false).unwrap_err();
    assert!(err.to_string().contains("inside output"), "got: {err}");
}

// 同名前缀兄弟目录（/photos vs /photos_backup）不构成重叠，不得误拒。
#[test]
fn copy_allows_sibling_dir_with_common_name_prefix() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("photos");
    let out = dir.path().join("photos_backup");
    fs::create_dir_all(&src).unwrap();
    tc::copy_png_to(&src, "a.png").unwrap();
    let report = run(&src, &out, false).unwrap();
    assert_eq!(report.copied, 1);
}

// 就地归档（output ⊂ source）第二次 move：已归档文件必须从 source 索引排除，
// 旧行为会把 archive 内文件判为自身副本并删除（数据丢失）。
#[test]
fn move_in_place_second_run_keeps_archived_files() {
    let dir = tempdir().unwrap();
    tc::copy_png_to(dir.path(), "a.png").unwrap();
    let out = dir.path().join("archive");

    let first = run(dir.path(), &out, true).unwrap();
    assert_eq!(first.copied, 1);
    let archived: Vec<_> = walkdir_files(&out);
    assert_eq!(archived.len(), 1, "first move must archive the file");

    let second = run(dir.path(), &out, true).unwrap();
    assert_eq!(
        second.scanned, 0,
        "archived files must be excluded from source"
    );
    let still_there: Vec<_> = walkdir_files(&out);
    assert_eq!(
        still_there.len(),
        1,
        "second move must not delete archived files"
    );
}

// 就地归档第二次 copy：source 仍含原文件（copy 不删），archive 子树被排除后
// scanned 只计源文件本身；与 archive 内副本 dedup 后 ignored。
#[test]
fn copy_in_place_second_run_excludes_output_subtree_from_scan() {
    let dir = tempdir().unwrap();
    tc::copy_png_to(dir.path(), "a.png").unwrap();
    let out = dir.path().join("archive");

    let first = run(dir.path(), &out, false).unwrap();
    assert_eq!(first.copied, 1);

    let second = run(dir.path(), &out, false).unwrap();
    assert_eq!(
        second.scanned, 1,
        "output subtree must not be re-scanned as source"
    );
    assert_eq!(second.copied, 0);
    assert_eq!(second.ignored, 1);
    assert_eq!(walkdir_files(&out).len(), 1, "no duplicate copy may appear");
}

// copy_with_sidecar 带 provider：build_source_index 的 enrich_candidates 注入
// 路径（生产 dispatch 只走此口）。provider 语义断言见 tests/lib_tidy/archive.rs
// 的 sidecar e2e；此处仅钉单测 binary 内的注入分支。
#[test]
fn copy_with_provider_runs_enrich_candidates() {
    fn no_candidates(
        _: &Location,
        _: &Arc<dyn Backend>,
    ) -> Vec<crate::entities::media_time::Candidate> {
        Vec::new()
    }
    let dir = tempdir().unwrap();
    tc::copy_png_to(dir.path(), "a.png").unwrap();
    let out = tempdir().unwrap();

    let report = copy_with_sidecar(
        &[local_source(dir.path())],
        local_source(out.path()),
        /* dry_run = */ false,
        /* remove = */ false,
        /* include_non_media = */ false,
        None,
        None,
        Some(no_candidates),
    )
    .unwrap();
    assert_eq!(report.copied, 1);
}

// canonical_prefix：Local 路径 canonicalize；不存在的路径回退原始串。
#[test]
fn canonical_prefix_falls_back_for_missing_local_path() {
    let loc = Location::Local(Utf8PathBuf::from("/no/such/dir/xyz"));
    assert_eq!(canonical_prefix(&loc), "/no/such/dir/xyz");
}

#[test]
fn canonical_prefix_uses_display_for_remote() {
    let loc = Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from("x"),
    };
    assert_eq!(canonical_prefix(&loc), loc.display());
}

// 递归收集目录下所有文件路径（断言 archive 内容用）。
fn walkdir_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}
