use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use tempfile::TempPath;
use tracing::debug;

use crate::entities::backend::{Backend, Entry, EntryKind};
use crate::entities::uri::Location;
use crate::usecases::config::config;
use crate::usecases::report::FEATURE_VERIFY;

#[path = "manifest_io.rs"]
pub(crate) mod manifest_io;
#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;

type Slots = Arc<Mutex<HashMap<String, Slot>>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Row {
    path: String,
    size: u64,
}

enum Slot {
    Idle,
    Recording,
    Replay { temp: TempPath, rows: u64 },
}

pub(crate) struct ScanCache {
    min: u64,
    slots: Option<Slots>,
}

enum Plan {
    Record(Slots, String),
    Replay(manifest_io::Reader),
    Live,
}

pub(crate) enum EntryStream<'a> {
    Live(Live<'a>),
    Replay(Replay),
}

pub(crate) struct Live<'a> {
    inner: Box<dyn Iterator<Item = io::Result<Entry>> + Send + 'a>,
    rec: Option<(manifest_io::Recorder, Slots, String, u64)>,
}

pub(crate) struct Replay {
    reader: manifest_io::Reader,
    root: Location,
}

impl Row {
    fn of(entry: &io::Result<Entry>) -> Option<Self> {
        entry
            .as_ref()
            .ok()
            .filter(|entry| entry.kind == EntryKind::File)
            .map(|entry| Self {
                path: entry.location.display(),
                size: entry.size,
            })
    }

    fn entry(&self, root: &Location) -> io::Result<Entry> {
        let location = match root {
            Location::Local(_) => Location::Local(Utf8PathBuf::from(self.path.as_str())),
            Location::Smb { .. } => Location::parse(self.path.as_str()).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("internal: {e}"))
            })?,
        };
        Ok(Entry {
            location,
            size: self.size,
            kind: EntryKind::File,
        })
    }
}

#[must_use]
pub(crate) fn keeps(rows: u64, min: u64) -> bool {
    rows >= min
}

#[cfg(test)]
thread_local! {
    static MIN_OVERRIDE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

fn manifest_min_entries() -> u64 {
    #[cfg(test)]
    if let Some(entries) = MIN_OVERRIDE.get() {
        return entries;
    }
    config().verify.manifest_min_entries
}

#[cfg(test)]
pub(crate) fn with_manifest_min_entries<R>(entries: u64, f: impl FnOnce() -> R) -> R {
    MIN_OVERRIDE.set(Some(entries));
    let result = f();
    MIN_OVERRIDE.set(None);
    result
}

impl ScanCache {
    #[must_use]
    pub(crate) fn off() -> Self {
        Self {
            min: u64::MAX,
            slots: None,
        }
    }

    #[must_use]
    pub(crate) fn on() -> Self {
        Self {
            min: manifest_min_entries(),
            slots: Some(Arc::new(Mutex::new(HashMap::new()))),
        }
    }

    pub(crate) fn entries<'a>(
        &'a self,
        root: &Location,
        backend: &'a Arc<dyn Backend>,
    ) -> EntryStream<'a> {
        let Some(slots) = &self.slots else {
            return EntryStream::Live(Live {
                inner: backend.walk(root),
                rec: None,
            });
        };
        let key = root.display();
        let mut map = slots.lock().expect("internal: manifest slot lock");
        let plan = match map.get(&key) {
            None => Plan::Record(Arc::clone(slots), key),
            Some(Slot::Replay { temp, rows }) => {
                manifest_io::open(temp, *rows).map_or(Plan::Live, Plan::Replay)
            }
            Some(Slot::Recording | Slot::Idle) => Plan::Live,
        };
        match plan {
            Plan::Record(owner, key) => {
                map.insert(key.clone(), Slot::Recording);
                drop(map);
                let rec =
                    manifest_io::Recorder::new().map(|recorder| (recorder, owner, key, self.min));
                EntryStream::Live(Live {
                    inner: backend.walk(root),
                    rec,
                })
            }
            Plan::Replay(reader) => {
                drop(map);
                EntryStream::Replay(Replay {
                    reader,
                    root: root.clone(),
                })
            }
            Plan::Live => {
                drop(map);
                EntryStream::Live(Live {
                    inner: backend.walk(root),
                    rec: None,
                })
            }
        }
    }
}

impl Iterator for EntryStream<'_> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Live(live) => live.next(),
            Self::Replay(replay) => replay.next(),
        }
    }
}

impl Live<'_> {
    fn seal(&mut self) {
        let Some((recorder, slots, key, min)) = self.rec.take() else {
            return;
        };
        let keep = keeps(recorder.rows(), min);
        let replay = recorder.finish(keep);
        if let Some((_, rows)) = &replay {
            debug!(
                feature = FEATURE_VERIFY,
                operation = "manifest",
                result = "recorded",
                root = %key,
                rows,
                "directory manifest recorded for grouped replay"
            );
        }
        let Ok(mut map) = slots.lock() else { return };
        match replay {
            Some((temp, rows)) => {
                map.insert(key, Slot::Replay { temp, rows });
            }
            None => {
                map.insert(key, Slot::Idle);
            }
        }
    }
}

impl Iterator for Live<'_> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(item) = self.inner.next() else {
            self.seal();
            return None;
        };
        if let Some((recorder, ..)) = &mut self.rec
            && let Some(row) = Row::of(&item)
        {
            recorder.push(&row);
        }
        Some(item)
    }
}

impl Drop for Live<'_> {
    fn drop(&mut self) {
        let Some((_, slots, key, _)) = self.rec.take() else {
            return;
        };
        if let Ok(mut map) = slots.lock() {
            map.insert(key, Slot::Idle);
        }
    }
}

impl Iterator for Replay {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.next_row() {
            Ok(Some(row)) => Some(row.entry(&self.root)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}
