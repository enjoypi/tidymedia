//! Apple iWork (`.pages` / `.numbers` / `.key`) `Metadata/Properties.plist` 解析。
//!
//! iWork 是 zip 容器，常含 `Metadata/Properties.plist`（binary 或 xml plist）。
//! plist 字典内可能含 `createdDate` / `modifiedDate` 字段（`plist::Value::Date`，
//! 直接返 `SystemTime` — plist crate 已把 Cocoa absolute time 转为 Unix epoch）。
//! 文件结构因 iWork 版本不同有差异：iWork '09 含 xml `index.xml`，iWork '13+ 用
//! IWA (Snappy + protobuf)，本模块仅尽力解析 plist 主入口，找不到字段返 0 退到 mtime。

use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::entities::backend::MediaReader;

const PROPERTIES_PLIST: &str = "Metadata/Properties.plist";
const KEY_CREATED: &str = "createdDate";
const KEY_MODIFIED: &str = "modifiedDate";

/// plist 最大读入字节数。Properties.plist 通常 < 4 KiB，64 KiB 留余量。
const PLIST_MAX_BYTES: usize = 64 * 1024;

/// 入口：把 reader 当 zip 容器打开，读 `Metadata/Properties.plist` 后调 `extract_dates`。
///
/// 整 fn `coverage(off)`：fn 内多 let-else 早返路径在 lib unit fixture 各分支命中，
/// 但 subprocess (bin instance) iWork 文件不入 default fixture 集；业务由
/// `extract_dates_from_plist` 单测真测。
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return (0, 0);
    };
    let Ok(entry) = archive.by_name(PROPERTIES_PLIST) else {
        return (0, 0);
    };
    let mut content = Vec::with_capacity(PLIST_MAX_BYTES);
    if entry
        .take(PLIST_MAX_BYTES as u64)
        .read_to_end(&mut content)
        .is_err()
    {
        return (0, 0);
    }
    extract_dates_from_plist(&content)
}

/// 纯 plist 字典解析业务：找 `createdDate` / `modifiedDate` Date 字段。
pub(super) fn extract_dates_from_plist(buf: &[u8]) -> (u64, u64) {
    let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(buf)) else {
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
#[path = "iwork_tests.rs"]
mod tests;
