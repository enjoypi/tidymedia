use std::time::{SystemTime, UNIX_EPOCH};

use crate::entities::backend::MediaReader;

use super::plist_io;
use super::zip_io;

const PROPERTIES_PLIST: &str = "Metadata/Properties.plist";
const KEY_CREATED: &str = "createdDate";
const KEY_MODIFIED: &str = "modifiedDate";
const PLIST_MAX_BYTES: usize = 64 * 1024;

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Some(mut archive) = zip_io::open_zip(reader) else {
        return (0, 0);
    };
    let Some(content) = zip_io::read_entry(&mut archive, PROPERTIES_PLIST, PLIST_MAX_BYTES as u64)
    else {
        return (0, 0);
    };
    extract_dates_from_plist(&content)
}

#[doc(hidden)]
pub fn extract_dates_from_plist(buf: &[u8]) -> (u64, u64) {
    let Some(value) = plist_io::read_value(buf) else {
        return (0, 0);
    };
    let Some(dict) = value.as_dictionary() else {
        return (0, 0);
    };
    let created = dict
        .get(KEY_CREATED)
        .and_then(plist::Value::as_date)
        .and_then(|d| systemtime_to_epoch(d.into()))
        .unwrap_or(0);
    let modified = dict
        .get(KEY_MODIFIED)
        .and_then(plist::Value::as_date)
        .and_then(|d| systemtime_to_epoch(d.into()))
        .unwrap_or(0);
    (created, modified)
}

fn systemtime_to_epoch(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

#[cfg(test)]
#[path = "iwork_deep_tests.rs"]
mod deep_tests;

#[cfg(test)]
#[path = "iwork_tests.rs"]
mod tests;
