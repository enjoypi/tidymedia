//! Windows same-volume fast-path：`subst` / NTFS junction 构造的「不同前缀同卷」
//! 路径必须命中 `do_copy` 的 `fs::rename` fast-path，不被误判为跨盘。
//!
//! 整文件 `#![cfg(windows)]`。`subst` (subst.exe) 与 `mklink /J` (cmd.exe builtin)
//! 都**不要管理员权限**，用户本地 Windows 机直接
//! `cargo nextest run --release windows_same_volume` 可跑。
//!
//! 验证手段：用 `filetime::set_file_mtime` 把 src 钉到固定时间戳，move 后断言
//! dst mtime 不变 —— `stream_copy` 路径会让 dst mtime = now，rename 路径会保留 src 原
//! mtime，是 fast-path 命中的强证据。
//!
//! `subst` 盘字是全局资源，本文件测试通过 `nextest.toml` 的 `windows-volume-mut`
//! test-group 强制 max-threads=1 串行。

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use tempfile::tempdir;
use tidymedia::{CommandResult, Commands, tidy_with};

use super::{DATA_DIR, FakeBackendFactory, local};

const PINNED_SECS: u64 = 1_704_067_200; // 2024-01-01 UTC
const FIXTURE: &str = "sample-with-offset.jpg";

fn copy_fixture(dst: &Path) {
    std::fs::copy(format!("{DATA_DIR}/{FIXTURE}"), dst).expect("copy fixture");
}

fn pin_mtime(path: &Path) {
    let pinned = filetime::FileTime::from_unix_time(PINNED_SECS.cast_signed(), 0);
    filetime::set_file_mtime(path, pinned).expect("set mtime");
}

fn read_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .expect("dst metadata")
        .modified()
        .expect("mtime")
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("mtime after epoch")
        .as_secs()
}

fn find_unused_drive_letter() -> Option<char> {
    for letter in (b'D'..=b'Z').rev() {
        let c = letter as char;
        if !Path::new(&format!("{c}:\\")).exists() {
            return Some(c);
        }
    }
    None
}

// Drop 时 `subst Y: /D` 释放盘字。panic 路径中错误吞掉避免二次 panic。
struct SubstGuard(char);
impl Drop for SubstGuard {
    fn drop(&mut self) {
        let _ = Command::new("subst")
            .args([format!("{}:", self.0).as_str(), "/D"])
            .output();
    }
}

fn move_cmd(src: &Path, out: &Path) -> Commands {
    Commands::Move {
        dry_run: false,
        include_non_media: false,
        sources: vec![local(src.to_str().unwrap())],
        output: local(out.to_str().unwrap()),
        archive_template: None,
        report: None,
    }
}

// subst Y: <tempdir> 把 tempdir 挂为 Y:\，通过 Y:\src 访问真实 tempdir 下的 src
// 子目录——两者路径前缀不同但同 NTFS 卷，fast-path 必须命中且保留 mtime。
#[test]
fn move_via_subst_same_volume_preserves_mtime() {
    let root = tempdir().unwrap();
    let Some(letter) = find_unused_drive_letter() else {
        eprintln!("windows_same_volume: skipped (no free drive letter)");
        return;
    };
    let status = Command::new("subst")
        .args([format!("{letter}:").as_str(), root.path().to_str().unwrap()])
        .status()
        .expect("run subst");
    if !status.success() {
        eprintln!("windows_same_volume: skipped (subst returned {status})");
        return;
    }
    let _guard = SubstGuard(letter);

    let src_dir_via_subst: PathBuf = format!(r"{letter}:\src").into();
    let real_src_dir = root.path().join("src");
    std::fs::create_dir(&real_src_dir).unwrap();
    let real_src_file = real_src_dir.join(FIXTURE);
    copy_fixture(&real_src_file);
    pin_mtime(&real_src_file);

    let out_dir = root.path().join("out");

    let factory = FakeBackendFactory::new();
    let r = tidy_with(&factory, move_cmd(&src_dir_via_subst, &out_dir)).expect("move via subst");
    let CommandResult::Copy(report) = r else {
        panic!("expected Copy report");
    };
    assert_eq!(report.copied, 1, "{report:?}");
    assert!(!real_src_file.exists(), "src must be removed");

    let archived = out_dir.join("2024").join("05").join(FIXTURE);
    assert!(archived.exists(), "dst missing at {archived:?}");
    assert_eq!(
        read_secs(&archived),
        PINNED_SECS,
        "subst same-volume rename must preserve mtime"
    );
}

// mklink /J link actual：通过 NTFS junction 的路径前缀访问同卷下的真实子目录。
#[test]
fn move_via_junction_same_volume_preserves_mtime() {
    let root = tempdir().unwrap();
    let actual_dir = root.path().join("actual");
    std::fs::create_dir(&actual_dir).unwrap();
    let real_src_file = actual_dir.join(FIXTURE);
    copy_fixture(&real_src_file);
    pin_mtime(&real_src_file);

    let junction = root.path().join("link");
    let status = Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            junction.to_str().unwrap(),
            actual_dir.to_str().unwrap(),
        ])
        .status()
        .expect("run mklink");
    if !status.success() {
        eprintln!("windows_same_volume: skipped (mklink returned {status})");
        return;
    }

    let out_dir = root.path().join("out");

    let factory = FakeBackendFactory::new();
    let r = tidy_with(&factory, move_cmd(&junction, &out_dir)).expect("move via junction");
    let CommandResult::Copy(report) = r else {
        panic!("expected Copy report");
    };
    assert_eq!(report.copied, 1, "{report:?}");

    let archived = out_dir.join("2024").join("05").join(FIXTURE);
    assert!(archived.exists(), "dst missing at {archived:?}");
    assert_eq!(
        read_secs(&archived),
        PINNED_SECS,
        "junction same-volume rename must preserve mtime"
    );
}
