//! 报告输出 Gateway：把 [`ReportSink`] 的写盘需求落到 JSON + 原子 rename。
//! 协议（JSON）与 IO 细节（tempfile + persist）封装在此层，usecases 通过 trait 注入。

use tracing::warn;

use crate::entities::common;
use crate::usecases::report::{Report, ReportSink};

const FEATURE_COPY: &str = "copy";
const FEATURE_FIND: &str = "find";

/// 把报告原子写到 `path`（先写临时文件再 persist）。
/// 写盘失败仅 warn，不阻断主流程。
pub struct JsonFileReportSink {
    pub path: String,
}

impl JsonFileReportSink {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    /// 公开包装：让 dispatch.rs 等不持 `&dyn ReportSink` 的位置直接调用同一实现。
    pub fn write(&self, report: &Report<'_>) {
        <Self as ReportSink>::write(self, report);
    }
}

impl ReportSink for JsonFileReportSink {
    fn write(&self, report: &Report<'_>) {
        match report {
            Report::Copy(r) => write_report_json(&self.path, *r, FEATURE_COPY),
            Report::Find(r) => write_report_json(&self.path, *r, FEATURE_FIND),
        }
    }
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

// 写临时文件 + rename 原子替换；语义由 sink_writes_valid_copy_json 断言。
fn try_write_report_json<T: serde::Serialize>(path: &str, report: &T) -> common::Result<()> {
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
    Ok(())
}

#[cfg(test)]
#[path = "report_sink_tests.rs"]
mod tests;
