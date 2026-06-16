use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::compute_output_prefix;
use super::find_duplicates;
use super::render_script;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::file_index::DuplicateGroup;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;

fn local_data_dir() -> (Location, Arc<dyn Backend>) {
    (
        Location::Local(Utf8PathBuf::from(tc::DATA_DIR)),
        LocalBackend::arc(),
    )
}

fn local_dir(p: &std::path::Path) -> (Location, Arc<dyn Backend>) {
    (
        Location::Local(Utf8PathBuf::from(p.to_str().unwrap())),
        LocalBackend::arc(),
    )
}

fn run_render(same: &[DuplicateGroup], prefix: Option<&str>) -> String {
    let mut sink: Vec<u8> = Vec::new();
    render_script(same, prefix, &mut sink);
    String::from_utf8(sink).unwrap()
}

// sample 已按 size 降序（render_script 不再 iter().rev()，调用方负责排序）。
fn sample_two_groups() -> Vec<DuplicateGroup> {
    vec![
        DuplicateGroup {
            size: 200,
            paths: vec![
                Utf8PathBuf::from("/data/big_a"),
                Utf8PathBuf::from("/data/big_b"),
            ],
        },
        DuplicateGroup {
            size: 100,
            paths: vec![
                Utf8PathBuf::from("/data/small_a"),
                Utf8PathBuf::from("/data/small_b"),
            ],
        },
    ]
}

#[test]
fn render_script_python_no_output_all_commented() {
    let same = sample_two_groups();
    let out = run_render(&same, None);
    // 头部 shebang + import
    assert!(out.starts_with("#!/usr/bin/env python3\n"));
    assert!(out.contains("import os\n"));
    // 无 output prefix → 所有 os.remove 都注释保护
    assert!(out.contains("# SIZE 200\n"));
    assert!(out.contains("# os.remove(\"/data/big_a\")\n"));
    assert!(out.contains("# os.remove(\"/data/big_b\")\n"));
    assert!(out.contains("# SIZE 100\n"));
    assert!(out.contains("# os.remove(\"/data/small_a\")\n"));
    assert!(out.contains("# os.remove(\"/data/small_b\")\n"));
    // 不应出现未注释的 os.remove
    for line in out.lines() {
        assert!(
            !line.starts_with("os.remove"),
            "no-prefix mode must comment all: {line}"
        );
    }
}

#[test]
fn render_script_uncommments_paths_outside_output_prefix() {
    let same = sample_two_groups();
    let out = run_render(&same, Some("/keepers"));
    // /data/* 不在 /keepers 下 → 待删（无注释）
    for path in [
        "/data/big_a",
        "/data/big_b",
        "/data/small_a",
        "/data/small_b",
    ] {
        let line = format!("os.remove(\"{path}\")");
        assert!(out.contains(&line), "missing line: {line}\nfull:\n{out}");
    }
    assert!(!out.contains("# os.remove(\"/data/"));
}

/// `/photos_backup` 不应被 `/photos` prefix 误判为「在 output 内须保留」。
/// 修复前：`path.starts_with("/photos")` 直接 true → 被注释 → 漏删；修复后：分隔符校验 → 待删。
#[test]
fn render_script_prefix_does_not_match_sibling_with_same_prefix() {
    let groups = vec![DuplicateGroup {
        size: 42,
        paths: vec![
            Utf8PathBuf::from("/photos/img.jpg"),
            Utf8PathBuf::from("/photos_backup/img.jpg"),
        ],
    }];
    let out = run_render(&groups, Some("/photos"));
    // /photos/img.jpg 在 output 下 → 被注释保护
    assert!(out.contains("# os.remove(\"/photos/img.jpg\")\n"));
    // /photos_backup/img.jpg 不在 output 下 → 待删（非注释）
    assert!(out.contains("os.remove(\"/photos_backup/img.jpg\")\n"));
    assert!(!out.contains("# os.remove(\"/photos_backup/img.jpg\")"));
}

/// path 恰等 prefix（虽极不常见但 `under_prefix` 的 `rest.is_empty()` 分支需覆盖）。
#[test]
fn render_script_path_exactly_equal_prefix_is_under() {
    let groups = vec![DuplicateGroup {
        size: 1,
        paths: vec![Utf8PathBuf::from("/keepers"), Utf8PathBuf::from("/other/x")],
    }];
    let out = run_render(&groups, Some("/keepers"));
    assert!(out.contains("# os.remove(\"/keepers\")\n"));
}

#[test]
fn render_script_keeps_paths_under_output_prefix_commented() {
    let groups = vec![DuplicateGroup {
        size: 42,
        paths: vec![
            Utf8PathBuf::from("/keepers/a"),
            Utf8PathBuf::from("/other/b"),
        ],
    }];
    let out = run_render(&groups, Some("/keepers"));
    assert!(out.contains("# os.remove(\"/keepers/a\")\n"));
    assert!(out.contains("os.remove(\"/other/b\")\n"));
    assert!(!out.contains("# os.remove(\"/other/b\")"));
}

#[test]
fn render_script_descending_size_order() {
    let same = sample_two_groups();
    let out = run_render(&same, None);
    let idx_200 = out.find("SIZE 200").unwrap();
    let idx_100 = out.find("SIZE 100").unwrap();
    assert!(idx_200 < idx_100);
}

#[test]
fn render_script_empty_input_writes_nothing() {
    let empty: Vec<DuplicateGroup> = Vec::new();
    let out = run_render(&empty, None);
    assert!(out.is_empty(), "empty input must skip header: {out:?}");
}

/// Windows 路径（含 `\`）必须按 Python 字符串字面量正确转义。
#[test]
fn render_script_escapes_windows_backslash_path() {
    let groups = vec![DuplicateGroup {
        size: 1,
        paths: vec![Utf8PathBuf::from(r"C:\Users\u\dup.jpg")],
    }];
    let out = run_render(&groups, None);
    assert!(
        out.contains(r#"# os.remove("C:\\Users\\u\\dup.jpg")"#),
        "windows backslash must escape to \\\\: {out}"
    );
}

/// 路径含 `"` 双引号也按 `\"` 转义，避免破坏 Python 字面量。
#[test]
fn render_script_escapes_double_quote_in_path() {
    let groups = vec![DuplicateGroup {
        size: 1,
        paths: vec![Utf8PathBuf::from(r#"/tmp/with"quote.jpg"#)],
    }];
    let out = run_render(&groups, None);
    assert!(
        out.contains(r#"# os.remove("/tmp/with\"quote.jpg")"#),
        "double-quote must escape: {out}"
    );
}

/// 同 size 不同 content 的两组重复集必须独立保留（旧 `BTreeMap<size, _>` 实现会覆盖）。
#[test]
fn search_same_preserves_distinct_groups_with_identical_size() {
    use std::fs;
    let dir = tempdir().unwrap();
    // 两对 4KiB 文件：a1=a2（首字节 'A'），b1=b2（首字节 'B'），全 1000 字节。
    let make = |name: &str, fill: u8| {
        let p = dir.path().join(name);
        let bytes = vec![fill; 1000];
        fs::write(&p, &bytes).unwrap();
        p
    };
    let a1 = make("a1.bin", b'A');
    let a2 = make("a2.bin", b'A');
    let b1 = make("b1.bin", b'B');
    let b2 = make("b2.bin", b'B');

    let mut idx = crate::entities::file_index::Index::new();
    idx.insert(a1.to_str().unwrap()).unwrap();
    idx.insert(a2.to_str().unwrap()).unwrap();
    idx.insert(b1.to_str().unwrap()).unwrap();
    idx.insert(b2.to_str().unwrap()).unwrap();

    // fast & secure 两种路径均必须返回 2 组（旧实现只剩 1 组）
    let fast_groups = idx.fast_search_same();
    assert_eq!(
        fast_groups.len(),
        2,
        "fast: distinct content must yield 2 groups"
    );
    let secure_groups = idx.search_same();
    assert_eq!(
        secure_groups.len(),
        2,
        "secure: distinct content must yield 2 groups"
    );
    // 两组的 size 都是 1000
    for g in &fast_groups {
        assert_eq!(g.size, 1000);
        assert_eq!(g.paths.len(), 2);
    }
}

// output 指向文件（非目录）必须返回 Err：旧实现返回空报告 + exit 0，
// 与"无重复"不可区分，误导基于退出码做删除决策的脚本。
#[test]
fn find_duplicates_output_is_file_returns_err() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let out_loc = Location::Local(Utf8PathBuf::from(tmp.path().to_str().unwrap()));
    let out_pair = (out_loc, LocalBackend::arc());
    let err = find_duplicates(true, vec![local_data_dir()], Some(&out_pair)).unwrap_err();
    assert!(err.to_string().contains("not a directory"), "got: {err}");
}

#[test]
fn find_duplicates_output_missing_returns_err() {
    let out_loc = Location::Local(Utf8PathBuf::from("/no/such/dir/xyz"));
    let out_pair = (out_loc, LocalBackend::arc());
    let err = find_duplicates(true, vec![local_data_dir()], Some(&out_pair)).unwrap_err();
    assert!(err.to_string().contains("not a directory"), "got: {err}");
}

#[test]
fn find_duplicates_no_output_branch_runs() {
    find_duplicates(true, vec![local_data_dir()], None).unwrap();
}

#[test]
fn find_duplicates_with_output_branch_runs() {
    let dir = tempdir().unwrap();
    let out_pair = local_dir(dir.path());
    find_duplicates(false, vec![local_data_dir()], Some(&out_pair)).unwrap();
}

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
