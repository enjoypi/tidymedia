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
fn render_script_keeps_survivor_when_no_path_under_output_prefix() {
    // 数据全在 /data，无任何 path 落在 /keepers 下：fix #2 让每组第一份做 survivor
    // （注释保护），其余仍发待删。否则跑脚本删光 = 数据丢失。
    let same = sample_two_groups();
    let out = run_render(&same, Some("/keepers"));
    // SURVIVOR 标记 + 首份注释保护
    assert!(out.contains("# SURVIVOR (no copy under output)"));
    assert!(out.contains("# os.remove(\"/data/big_a\")\n"));
    assert!(out.contains("# os.remove(\"/data/small_a\")\n"));
    // 同组其余仍发待删
    assert!(out.contains("os.remove(\"/data/big_b\")\n"));
    assert!(out.contains("os.remove(\"/data/small_b\")\n"));
    // 但 big_b/small_b 自身不应被注释（出现 `# os.remove(".../*_b")` 即漏删）
    assert!(!out.contains("# os.remove(\"/data/big_b\")"));
    assert!(!out.contains("# os.remove(\"/data/small_b\")"));
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

#[test]
fn render_script_escapes_newline_in_path() {
    let groups = vec![DuplicateGroup {
        size: 1,
        paths: vec![Utf8PathBuf::from("/tmp/with\nnewline.jpg")],
    }];
    let out = run_render(&groups, None);
    assert!(
        out.contains(r#"# os.remove("/tmp/with\nnewline.jpg")"#),
        "newline must escape to \\n: {out}"
    );
    // 反向断言：生成脚本单行不含真换行（除末尾）
    let os_line = out
        .lines()
        .find(|l| l.contains("os.remove"))
        .expect("expect os.remove line");
    assert!(
        !os_line.contains('\n'),
        "os.remove line must be single-line: {os_line}"
    );
}

/// `\r` / `\t` / NUL 等控制字符走 `\xNN`（NUL 除外，`\t` 有专用字面量）；
/// 与 Python 字符串规范一致，避免 CRLF 或制表符打断脚本布局。
#[test]
fn render_script_escapes_control_chars_in_path() {
    let groups = vec![DuplicateGroup {
        size: 1,
        paths: vec![Utf8PathBuf::from("/tmp/a\rb\tc\x01d.jpg")],
    }];
    let out = run_render(&groups, None);
    assert!(out.contains(r"\r"), "CR must escape to \\r: {out}");
    assert!(out.contains(r"\t"), "TAB must escape to \\t: {out}");
    assert!(out.contains(r"\x01"), "0x01 must escape to \\x01: {out}");
}

/// DEL 字符 (0x7f) 也走 `\xNN`：`escape_py_string` guard `c < 0x20 || c == '\x7f'`
/// 的第二 sub-branch 需独立触达（`\x01` 只覆盖 `< 0x20`）。
#[test]
fn render_script_escapes_delete_char_in_path() {
    let groups = vec![DuplicateGroup {
        size: 1,
        paths: vec![Utf8PathBuf::from("/tmp/before\x7fafter.jpg")],
    }];
    let out = run_render(&groups, None);
    assert!(
        out.contains(r"\x7f"),
        "DEL (0x7f) must escape to \\x7f: {out}"
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

    let idx = crate::entities::file_index::Index::new();
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
