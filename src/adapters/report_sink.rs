//! 报告输出 Gateway：把 [`ReportSink`] 的写盘需求落到 JSON + 原子 rename。
//! 协议（JSON）与 IO 细节（tempfile + persist）封装在此层，usecases 通过 trait 注入。

use tracing::warn;

use crate::entities::common;
use crate::usecases::report::{Report, ReportSink};

const FEATURE_COPY: &str = "copy";
const FEATURE_FIND: &str = "find";
const FEATURE_MOVE_TEXT_SHOT: &str = "move_text_shot";
const FEATURE_CULL: &str = "cull";

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
            Report::MoveTextShot(r) => write_report_json(&self.path, *r, FEATURE_MOVE_TEXT_SHOT),
            Report::Cull(r) => write_report_json(&self.path, *r, FEATURE_CULL),
        }
    }
}

fn write_report_json<T: serde::Serialize>(path: &str, report: &T, feature: &str) {
    // CopyReport / FindReport / MoveTextShotReport 均为纯字段 derive(Serialize) 结构体，
    // 序列化不可能失败。
    let json = serde_json::to_string_pretty(report)
        .expect("internal error: serializing report must not fail");
    match try_write_report_json(path, json.as_bytes()) {
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

// 临时文件 + 原子 persist 的 object-safe 抽象：让 try_write_report_json 不再硬编码
// NamedTempFile，单测可注入 mock 实现触发 write/flush/persist 各自的 Err arm（real
// NamedTempFile 写盘失败在测试环境不可稳定触发）。`self: Box<Self>` 让 trait 对象
// 持有 sole ownership 并消耗 self（NamedTempFile::persist 签名要求）。
pub(crate) trait TempPersist {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
    fn persist_to(self: Box<Self>, path: &str) -> std::io::Result<()>;
}

impl TempPersist for tempfile::NamedTempFile {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        std::io::Write::write_all(self, buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(self)
    }
    fn persist_to(self: Box<Self>, path: &str) -> std::io::Result<()> {
        (*self).persist(path).map(|_| ()).map_err(|e| e.error)
    }
}

// 非泛型：所有调用方共享一份 instance，避免 generic monomorphization 让 llvm-cov
// 每份独立计 region 出现虚报。
fn try_write_report_json(path: &str, bytes: &[u8]) -> common::Result<()> {
    let parent = std::path::Path::new(path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let tmp = tempfile::NamedTempFile::new_in(parent)?;
    write_and_persist(path, bytes, Box::new(tmp))
}

fn write_and_persist(
    path: &str,
    bytes: &[u8],
    mut tmp: Box<dyn TempPersist>,
) -> common::Result<()> {
    tmp.write_all(bytes)?;
    tmp.flush()?;
    tmp.persist_to(path).map_err(common::Error::Io)?;
    Ok(())
}

#[cfg(test)]
#[path = "report_sink_tests.rs"]
mod tests;
