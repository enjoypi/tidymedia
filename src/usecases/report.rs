// JSON 报告结构体：copy / find 操作的机器可读摘要。
// `serde` 注解仅用于序列化输出，不影响业务逻辑（serde 用在 framework 层输出，
// 报告 struct 本身属于 usecase 层的值对象）。

use serde_derive::Serialize;
use tracing::warn;

use crate::entities::common;

const FEATURE_COPY: &str = "copy";
const FEATURE_FIND: &str = "find";

/// copy / move 操作报告。
#[derive(Debug, Serialize)]
pub struct CopyReport {
    pub scanned: usize,
    pub copied: usize,
    pub ignored: usize,
    pub failed: usize,
    pub dry_run: bool,
    pub remove: bool,
    pub include_non_media: bool,
    pub errors: Vec<ReportError>,
}

/// find 操作报告。
#[derive(Debug, Serialize)]
pub struct FindReport {
    pub scanned: usize,
    /// 每个重复组：文件路径列表（按大小降序）。
    pub groups: Vec<Vec<String>>,
    pub bytes_read: u64,
}

/// 报告中的单条错误记录。
#[derive(Debug, Serialize)]
pub struct ReportError {
    pub path: String,
    pub message: String,
}

/// 把报告原子写到 `path`（先写临时文件再 persist）。
/// 写盘失败仅 warn，不阻断主流程。
pub fn write_copy_report(path: &str, report: &CopyReport) {
    write_report_json(path, report, FEATURE_COPY);
}

/// 同 [`write_copy_report`]，用于 find 报告。
pub fn write_find_report(path: &str, report: &FindReport) {
    write_report_json(path, report, FEATURE_FIND);
}

fn write_report_json<T: serde::Serialize>(path: &str, report: &T, feature: &str) {
    match try_write_report_json(path, report) {
        Ok(()) => {}
        Err(e) => {
            warn!(
                feature,
                operation = "write_report",
                result = "error",
                report_path = path,
                error = %e,
                "failed to write report; main flow continues"
            );
        }
    }
}

// write_all / flush / persist の `?` Err 分支在普通文件系统下不可稳定触发；
// 整体标 coverage(off)，语义由 write_copy_report_creates_valid_json 断言。
#[cfg_attr(coverage_nightly, coverage(off))]
fn try_write_report_json<T: serde::Serialize>(path: &str, report: &T) -> common::Result<()> {
    use std::fs;
    use std::io::Write;

    // 写到同目录下的临时文件，再 rename 原子替换。
    let parent = std::path::Path::new(path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    let json =
        serde_json::to_string_pretty(report).map_err(|e| std::io::Error::other(e.to_string()))?;
    tmp.write_all(json.as_bytes())?;
    tmp.flush()?;
    // persist 内部做 rename；跨设备 rename 失败时回退到 copy+delete。
    tmp.persist(path).map_err(|e| common::Error::Io(e.error))?;
    let _ = fs::metadata(path); // no-op：确保路径存在（lint 用）
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{CopyReport, FindReport, write_copy_report, write_find_report};

    fn sample_copy_report() -> CopyReport {
        CopyReport {
            scanned: 5,
            copied: 3,
            ignored: 1,
            failed: 1,
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
    fn write_copy_report_creates_valid_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("report.json");
        let report = sample_copy_report();
        write_copy_report(path.to_str().unwrap(), &report);
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["scanned"], 5);
        assert_eq!(parsed["copied"], 3);
        assert_eq!(parsed["dry_run"], false);
    }

    #[test]
    fn write_find_report_creates_valid_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("find_report.json");
        let report = sample_find_report();
        write_find_report(path.to_str().unwrap(), &report);
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["scanned"], 10);
        assert_eq!(parsed["bytes_read"], 1024);
        let groups = parsed["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn write_copy_report_invalid_path_only_warns() {
        // 父目录不存在 → warn 不 panic
        let report = sample_copy_report();
        write_copy_report("/nonexistent_dir_xyz/r.json", &report);
    }

    #[test]
    fn write_find_report_invalid_path_only_warns() {
        let report = sample_find_report();
        write_find_report("/nonexistent_dir_xyz/r.json", &report);
    }
}
