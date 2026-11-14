use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::ser::{Error as _, SerializeSeq};
use serde::{Serialize, Serializer};
use tempfile::TempPath;

const SPILL_BUFFER_BYTES: usize = 64 << 10;

pub trait SpillKey {
    fn spill_key(&self) -> String;
}

struct Runs {
    dir: PathBuf,
    files: Vec<TempPath>,
}

type Lines = io::Lines<BufReader<std::fs::File>>;

impl Runs {
    fn push(&mut self, mut records: Vec<(String, String)>) -> io::Result<()> {
        records.sort_by(|a, b| a.0.cmp(&b.0));
        let file = tempfile::NamedTempFile::new_in(&self.dir)?;
        let mut out = BufWriter::with_capacity(SPILL_BUFFER_BYTES, file.as_file());
        let written = records
            .iter()
            .try_for_each(|(key, json)| {
                serde_json::to_writer(&mut out, key)
                    .map_err(io::Error::from)
                    .and_then(|()| writeln!(out, "\t{json}"))
            })
            .and_then(|()| out.flush());
        drop(out);
        written.map(|()| self.files.push(file.into_temp_path()))
    }

    fn for_each(&self, f: &mut dyn FnMut(&str) -> io::Result<()>) -> io::Result<()> {
        let mut readers: Vec<(Lines, Option<String>)> = Vec::with_capacity(self.files.len());
        let mut heap = BinaryHeap::new();
        for path in &self.files {
            let mut lines = open_lines(path)?;
            let idx = readers.len();
            let slot = next_record(&mut lines)?.map(|(key, json)| {
                heap.push(Reverse((key, idx)));
                json
            });
            readers.push((lines, slot));
        }
        while let Some(Reverse((_, idx))) = heap.pop() {
            let (lines, slot) = &mut readers[idx];
            let json = slot
                .take()
                .expect("internal: a queued run holds its next record");
            f(&json)?;
            if let Some((key, json)) = next_record(lines)? {
                heap.push(Reverse((key, idx)));
                *slot = Some(json);
            }
        }
        Ok(())
    }
}

fn open_lines(path: &Path) -> io::Result<Lines> {
    std::fs::File::open(path).map(|file| BufReader::with_capacity(SPILL_BUFFER_BYTES, file).lines())
}

fn next_record(lines: &mut Lines) -> io::Result<Option<(String, String)>> {
    let Some(line) = lines.next().transpose()? else {
        return Ok(None);
    };
    let (key, json) = line
        .split_once('\t')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "corrupt spill record"))?;
    let key: String = serde_json::from_str(key)?;
    Ok(Some((key, json.to_owned())))
}

pub struct Spill<T> {
    runs: Runs,
    marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Spill<T> {
    #[must_use]
    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            runs: Runs {
                dir,
                files: Vec::new(),
            },
            marker: std::marker::PhantomData,
        }
    }
}

impl<T> fmt::Debug for Spill<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Spill({} runs)", self.runs.files.len())
    }
}

impl<T> Default for Spill<T> {
    fn default() -> Self {
        Self::in_dir(spill_dir())
    }
}

impl<T: Serialize + DeserializeOwned + SpillKey> Spill<T> {
    pub fn push_run(&mut self, items: &[T]) -> io::Result<()> {
        items
            .iter()
            .map(|item| serde_json::to_string(item).map(|json| (item.spill_key(), json)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(io::Error::from)
            .and_then(|records| self.runs.push(records))
    }

    pub fn for_each(&self, f: &mut dyn FnMut(&T) -> io::Result<()>) -> io::Result<()> {
        self.runs.for_each(&mut |json| {
            serde_json::from_str(json)
                .map_err(io::Error::from)
                .and_then(|item: T| f(&item))
        })
    }
}

#[cfg(test)]
thread_local! {
    static SPILL_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn spill_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = SPILL_DIR.with_borrow(Clone::clone) {
        return dir;
    }
    std::env::temp_dir()
}

#[cfg(test)]
pub(crate) fn with_spill_dir<R>(dir: PathBuf, f: impl FnOnce() -> R) -> R {
    SPILL_DIR.set(Some(dir));
    let result = f();
    SPILL_DIR.set(None);
    result
}

pub struct Listed<T> {
    items: Vec<T>,
    spill: Option<Arc<Spill<T>>>,
}

impl<T> Listed<T> {
    #[must_use]
    pub fn spilled(spill: Spill<T>) -> Self {
        Self {
            items: Vec::new(),
            spill: Some(Arc::new(spill)),
        }
    }
}

impl<T: Serialize + DeserializeOwned + SpillKey> Listed<T> {
    pub fn for_each(&self, f: &mut dyn FnMut(&T) -> io::Result<()>) -> io::Result<()> {
        if let Some(spill) = &self.spill {
            return spill.for_each(f);
        }
        self.items.iter().try_for_each(f)
    }

    #[must_use]
    pub fn is_spilled(&self) -> bool {
        self.spill.is_some()
    }
}

impl<T> Default for Listed<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            spill: None,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Listed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.spill {
            Some(spill) => spill.fmt(f),
            None => self.items.fmt(f),
        }
    }
}

impl<T> From<Vec<T>> for Listed<T> {
    fn from(items: Vec<T>) -> Self {
        Self { items, spill: None }
    }
}

impl<T> Deref for Listed<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.items
    }
}

impl<T> DerefMut for Listed<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.items
    }
}

fn to_io(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

impl<T: Serialize + DeserializeOwned + SpillKey> Serialize for Listed<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_seq(None).and_then(|mut seq| {
            self.for_each(&mut |item| seq.serialize_element(item).map_err(to_io))
                .map_err(S::Error::custom)
                .and_then(|()| seq.end())
        })
    }
}

#[cfg(test)]
#[path = "spill_tests.rs"]
mod tests;
