#[path = "report_sink_io.rs"]
mod report_sink_io;

use tracing::warn;

use crate::entities::common;
use crate::usecases::report::{
    FEATURE_COPY, FEATURE_FIND, FEATURE_MOVE, FEATURE_MOVE_TEXT_SHOT, FEATURE_VERIFY, Report,
    ReportSink,
};

pub struct JsonFileReportSink {
    pub path: String,
}

impl JsonFileReportSink {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    pub fn write(&self, report: &Report<'_>) -> common::Result<()> {
        <Self as ReportSink>::write(self, report)
    }
}

impl ReportSink for JsonFileReportSink {
    fn write(&self, report: &Report<'_>) -> common::Result<()> {
        match report {
            Report::Copy(r) => {
                let feature = if r.remove { FEATURE_MOVE } else { FEATURE_COPY };
                write_report_json(&self.path, *r, feature)
            }
            Report::Find(r) => write_report_json(&self.path, *r, FEATURE_FIND),
            Report::MoveTextShot(r) => write_report_json(&self.path, *r, FEATURE_MOVE_TEXT_SHOT),
            Report::Verify(r) => write_report_json(&self.path, *r, FEATURE_VERIFY),
        }
    }
}

fn write_report_json<T: serde::Serialize>(
    path: &str,
    report: &T,
    feature: &str,
) -> common::Result<()> {
    let json = report_sink_io::serialize(report);
    report_sink_io::try_write(path, json.as_bytes()).map_err(|e| {
        let common::Error::Io(io_err) = e;
        let wrapped = std::io::Error::new(
            io_err.kind(),
            format!("cannot write report to {path}: {io_err}"),
        );
        warn!(
            feature,
            operation = "write_report",
            result = "error",
            report_path = path,
            error = %wrapped,
            "failed to write report"
        );
        common::Error::Io(wrapped)
    })
}

pub(crate) trait TempPersist {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
    fn persist_to(self: Box<Self>, path: &str) -> std::io::Result<()>;
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
