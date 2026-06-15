use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::SCRIPT_LINE_TAIL;
use super::comment;
use super::find_duplicates;
use super::render_script;
use super::rm;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::file_index::DuplicateGroup;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;

// 平台条件行尾常量的别名：`CR` 在 Windows 解析为 "\r"，其他平台为 ""，
// 让平台分支共用断言模板。
const CR: &str = SCRIPT_LINE_TAIL;

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

fn run_render(same: &[DuplicateGroup], prefix: Option<&str>, c: &str, r: &str) -> String {
    let mut sink: Vec<u8> = Vec::new();
    render_script(same, prefix, c, r, &mut sink);
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
fn render_script_unix_tokens_no_output() {
    let same = sample_two_groups();
    let out = run_render(&same, None, "#", "rm");
    let expected = format!(
        "#SIZE 200{CR}\n#rm \"/data/big_a\"{CR}\n#rm \"/data/big_b\"{CR}\n\n\
         #SIZE 100{CR}\n#rm \"/data/small_a\"{CR}\n#rm \"/data/small_b\"{CR}\n\n"
    );
    assert_eq!(out, expected);
}

#[test]
fn render_script_windows_tokens_no_output() {
    let same = sample_two_groups();
    let out = run_render(&same, None, ":", "DEL");
    assert!(out.contains(&format!(":SIZE 200{CR}\n")));
    assert!(out.contains(&format!(":DEL \"/data/big_a\"{CR}\n")));
}

// 平台分支显式锚定：Linux/macOS 输出 LF 行尾（无 `\r`）以保下游 `| sh` 可用；
// Windows 输出 CRLF（含 `\r`）以保 cmd.exe 风格脚本可用。
#[cfg(not(target_os = "windows"))]
#[test]
fn render_script_non_windows_uses_lf_line_endings() {
    let same = sample_two_groups();
    let out = run_render(&same, None, "#", "rm");
    assert!(
        !out.contains('\r'),
        "non-Windows output must not contain CR: {out:?}"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn render_script_windows_uses_crlf_line_endings() {
    let same = sample_two_groups();
    let out = run_render(&same, None, "#", "rm");
    assert!(
        out.contains("\r\n"),
        "Windows output must contain CRLF: {out:?}"
    );
}

#[test]
fn render_script_uncommments_paths_outside_output_prefix() {
    let same = sample_two_groups();
    let out = run_render(&same, Some("/keepers"), "#", "rm");
    for line in [
        format!("rm \"/data/big_a\"{CR}"),
        format!("rm \"/data/big_b\"{CR}"),
        format!("rm \"/data/small_a\"{CR}"),
        format!("rm \"/data/small_b\"{CR}"),
    ] {
        assert!(out.contains(&line), "missing line: {line}\nfull:\n{out}");
    }
    assert!(!out.contains("#rm \"/data/"));
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
    let out = run_render(&groups, Some("/photos"), "#", "rm");
    // /photos/img.jpg 在 output 下 → 被注释保护
    assert!(out.contains(&format!("#rm \"/photos/img.jpg\"{CR}")));
    // /photos_backup/img.jpg 不在 output 下 → 待删（非注释）
    assert!(out.contains(&format!("rm \"/photos_backup/img.jpg\"{CR}")));
    assert!(!out.contains("#rm \"/photos_backup/img.jpg\""));
}

/// path 恰等 prefix（虽极不常见但 `under_prefix` 的 `rest.is_empty()` 分支需覆盖）。
#[test]
fn render_script_path_exactly_equal_prefix_is_under() {
    let groups = vec![DuplicateGroup {
        size: 1,
        paths: vec![Utf8PathBuf::from("/keepers"), Utf8PathBuf::from("/other/x")],
    }];
    let out = run_render(&groups, Some("/keepers"), "#", "rm");
    assert!(out.contains(&format!("#rm \"/keepers\"{CR}")));
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
    let out = run_render(&groups, Some("/keepers"), "#", "rm");
    assert!(out.contains(&format!("#rm \"/keepers/a\"{CR}")));
    assert!(out.contains(&format!("rm \"/other/b\"{CR}")));
    assert!(!out.contains("#rm \"/other/b\""));
}

#[test]
fn render_script_descending_size_order() {
    let same = sample_two_groups();
    let out = run_render(&same, None, "#", "rm");
    let idx_200 = out.find("SIZE 200").unwrap();
    let idx_100 = out.find("SIZE 100").unwrap();
    assert!(idx_200 < idx_100);
}

#[test]
fn render_script_empty_input_writes_nothing() {
    let empty: Vec<DuplicateGroup> = Vec::new();
    let out = run_render(&empty, None, "#", "rm");
    assert!(out.is_empty());
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

#[test]
#[cfg(not(target_os = "windows"))]
fn comment_and_rm_unix_tokens() {
    assert_eq!(comment(), "#");
    assert_eq!(rm(), "rm");
}

#[test]
#[cfg(target_os = "windows")]
fn comment_and_rm_windows_tokens() {
    assert_eq!(comment(), ":");
    assert_eq!(rm(), "DEL");
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
