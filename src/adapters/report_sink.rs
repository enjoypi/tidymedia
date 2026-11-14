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
    report_sink_io::try_write(path, report).map_err(|e| {
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

struct PersistWriter<'a>(&'a mut dyn TempPersist);

impl std::io::Write for PersistWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write_all(buf).map(|()| buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

const REPORT_WRITE_BUFFER_BYTES: usize = 64 << 10;

fn write_and_persist<T: serde::Serialize + ?Sized>(
    path: &str,
    report: &T,
    mut tmp: Box<dyn TempPersist>,
) -> common::Result<()> {
    let mut writer =
        std::io::BufWriter::with_capacity(REPORT_WRITE_BUFFER_BYTES, PersistWriter(tmp.as_mut()));
    serde_json::to_writer_pretty(&mut writer, report).map_err(std::io::Error::from)?;
    std::io::Write::flush(&mut writer)?;
    drop(writer);
    tmp.persist_to(path).map_err(common::Error::Io)?;
    Ok(())
}

#[cfg(test)]
#[path = "report_sink_tests.rs"]
mod tests;
