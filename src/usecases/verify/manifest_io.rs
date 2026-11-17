use std::io::{self, BufRead, BufReader, BufWriter, Write};

use tempfile::{NamedTempFile, TempPath};
use tracing::warn;

use super::Row;
use crate::usecases::report::FEATURE_VERIFY;
use crate::usecases::spill::{SPILL_BUFFER_BYTES, spill_dir};

type Lines = io::Lines<BufReader<std::fs::File>>;

pub(crate) struct Recorder {
    file: NamedTempFile,
    out: BufWriter<std::fs::File>,
    rows: u64,
    failed: bool,
}

impl Recorder {
    pub(crate) fn new() -> Option<Self> {
        let file = NamedTempFile::new_in(spill_dir()).ok()?;
        let out = BufWriter::with_capacity(SPILL_BUFFER_BYTES, file.as_file().try_clone().ok()?);
        Some(Self {
            file,
            out,
            rows: 0,
            failed: false,
        })
    }

    pub(crate) fn rows(&self) -> u64 {
        self.rows
    }

    pub(crate) fn push(&mut self, row: &Row) {
        if self.failed {
            return;
        }
        let written = serde_json::to_writer(&mut self.out, row)
            .map_err(io::Error::from)
            .and_then(|()| writeln!(self.out));
        if written.is_err() {
            self.failed = true;
            self.rows = 0;
            warn!(
                feature = FEATURE_VERIFY,
                operation = "manifest",
                result = "write_failed",
                path = %self.file.path().display(),
                "directory manifest write failed; falling back to live walk",
            );
        } else {
            self.rows += 1;
        }
    }

    pub(crate) fn finish(mut self, keep: bool) -> Option<(TempPath, u64)> {
        let rows = self.rows;
        let flushed = self.out.flush().is_ok();
        drop(self.out);
        if !keep || !flushed || rows == 0 {
            return None;
        }
        Some((self.file.into_temp_path(), rows))
    }
}

pub(crate) struct Reader {
    lines: Lines,
    rows: u64,
    read: u64,
}

pub(crate) fn open(temp: &TempPath, rows: u64) -> io::Result<Reader> {
    let file = std::fs::File::open(temp)?;
    Ok(Reader {
        lines: BufReader::with_capacity(SPILL_BUFFER_BYTES, file).lines(),
        rows,
        read: 0,
    })
}

#[cfg(test)]
pub(crate) fn reader_from(text: &str, rows: u64) -> Reader {
    let file = tempfile::NamedTempFile::new().expect("internal: temp manifest for test");
    std::fs::write(file.path(), text).expect("internal: temp manifest write");
    let path = file.keep().expect("internal: temp manifest keep").1;
    Reader {
        lines: BufReader::with_capacity(SPILL_BUFFER_BYTES, std::fs::File::open(&path).unwrap())
            .lines(),
        rows,
        read: 0,
    }
}

impl Reader {
    pub(crate) fn next_row(&mut self) -> io::Result<Option<Row>> {
        let Some(line) = self.lines.next().transpose()? else {
            if self.read != self.rows {
                warn!(
                    feature = FEATURE_VERIFY,
                    operation = "manifest",
                    result = "short",
                    rows = self.rows,
                    read = self.read,
                    "directory manifest replay ended early",
                );
            }
            return Ok(None);
        };
        self.read += 1;
        serde_json::from_str(&line)
            .map(Some)
            .map_err(io::Error::from)
    }
}
