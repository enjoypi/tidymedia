use std::io;

use tempfile::tempdir;

use super::{JsonFileReportSink, TempPersist, write_and_persist};
use crate::usecases::report::{CopyReport, FindReport, Report};

struct WriteFails;
impl TempPersist for WriteFails {
    fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
        Err(io::Error::other("inject write fail"))
    }
    fn flush(&mut self) -> io::Result<()> {
        unreachable!("short-circuit on write Err")
    }
    fn persist_to(self: Box<Self>, _: &str) -> io::Result<()> {
        unreachable!("short-circuit on write Err")
    }
}

struct FlushFails;
impl TempPersist for FlushFails {
    fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("inject flush fail"))
    }
    fn persist_to(self: Box<Self>, _: &str) -> io::Result<()> {
        unreachable!("short-circuit on flush Err")
    }
}

struct PersistFails;
impl TempPersist for PersistFails {
    fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn persist_to(self: Box<Self>, _: &str) -> io::Result<()> {
        Err(io::Error::other("inject persist fail"))
    }
}

fn sample_copy_report() -> CopyReport {
    CopyReport {
        scanned: 5,
        copied: 3,
        ignored: 1,
        failed: 1,
        skipped_empty: 0,
        skipped_unreadable: 0,
        walker_errors: 0,
        dry_run: false,
        remove: false,
        include_non_media: false,
        errors: vec![],
    }
}

fn sample_find_report() -> FindReport {
    FindReport {
        scanned: 10,
        groups: vec![vec!["a.jpg".into(), "a_copy.jpg".into()]],
        bytes_read: 1024,
    }
}

#[test]
fn sink_writes_valid_copy_json() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("report.json");
    let sink = JsonFileReportSink::new(path.to_str().unwrap());
    let r = sample_copy_report();
    sink.write(&Report::Copy(&r));
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["scanned"], 5);
    assert_eq!(parsed["copied"], 3);
    assert_eq!(parsed["dry_run"], false);
}

#[test]
fn sink_writes_valid_find_json() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("find_report.json");
    let sink = JsonFileReportSink::new(path.to_str().unwrap());
    let r = sample_find_report();
    sink.write(&Report::Find(&r));
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["scanned"], 10);
    assert_eq!(parsed["bytes_read"], 1024);
    let groups = parsed["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
}

#[test]
fn sink_copy_invalid_path_only_warns() {
    // 父目录不存在 → warn 不 panic
    let sink = JsonFileReportSink::new("/nonexistent_dir_xyz/r.json");
    let r = sample_copy_report();
    sink.write(&Report::Copy(&r));
}

#[test]
fn sink_find_invalid_path_only_warns() {
    let sink = JsonFileReportSink::new("/nonexistent_dir_xyz/r.json");
    let r = sample_find_report();
    sink.write(&Report::Find(&r));
}

// `new_in(parent)` 在父目录可写时成功 → 命中 line 68 `persist(...)` 路径。
// target 指向已存在的非空目录时 Linux `rename(file, dir)` 返 EISDIR，触发 `map_err` 闭包。
// 必须 Copy 与 Find 各一份：write_report_json::<CopyReport> 与 ::<FindReport> 是独立 monomorphization。
#[test]
fn sink_copy_persist_fails_when_target_is_directory() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("conflict");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("placeholder"), b"x").unwrap();
    let sink = JsonFileReportSink::new(target.to_str().unwrap());
    sink.write(&Report::Copy(&sample_copy_report()));
    assert!(target.is_dir());
}

#[test]
fn sink_find_persist_fails_when_target_is_directory() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("conflict");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("placeholder"), b"x").unwrap();
    let sink = JsonFileReportSink::new(target.to_str().unwrap());
    sink.write(&Report::Find(&sample_find_report()));
    assert!(target.is_dir());
}

#[test]
fn write_and_persist_propagates_write_err() {
    let err = write_and_persist("ignored", b"{}", Box::new(WriteFails)).unwrap_err();
    assert!(err.to_string().contains("inject write fail"), "{err}");
}

#[test]
fn write_and_persist_propagates_flush_err() {
    let err = write_and_persist("ignored", b"{}", Box::new(FlushFails)).unwrap_err();
    assert!(err.to_string().contains("inject flush fail"), "{err}");
}

#[test]
fn write_and_persist_propagates_persist_err() {
    let err = write_and_persist("ignored", b"{}", Box::new(PersistFails)).unwrap_err();
    assert!(err.to_string().contains("inject persist fail"), "{err}");
}
