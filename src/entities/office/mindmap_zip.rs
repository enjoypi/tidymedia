//! 思维导图 zip 容器解析。
//!
//! 首版仅支持 xmind 6+ `metadata.json`（`{"created": <ms>, "modified": <ms>}`）。
//! itmz / mindnode / mmap (`MindManager`) 子格式各有不同结构，YAGNI 后续 commit 接入。
//!
//! 入口与复杂业务 fn 含 per-instance phantom branch 缺口，独立到 `phantom` 子模块供
//! ignore-regex 排除（CLAUDE.md office 子模块套路）；本文件保留容器常量与 metadata 结构。

use serde::Deserialize;

use super::{MIME_XMIND, MIME_XMIND_ALT};

const METADATA_JSON: &str = "metadata.json";
const JSON_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
struct XmindMetadata {
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    modified: Option<u64>,
}

/// topic 内容文件：xmind classic 是 `content.xml`、xmind zen (8+) 是 `content.json`。
const CONTENT_XML: &str = "content.xml";
const CONTENT_JSON: &str = "content.json";
const CONTENT_MAX_BYTES: u64 = 256 * 1024;

#[path = "mindmap_zip_phantom.rs"]
mod phantom;

#[doc(hidden)]
pub use self::phantom::{
    collect_json_titles, extract_dates_from_json, extract_text, find_subslice, millis_to_secs,
    parse, parse_xmind, read_entry_capped,
};

#[cfg(test)]
#[path = "mindmap_zip_tests.rs"]
mod tests;
