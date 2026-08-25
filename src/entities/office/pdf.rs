//! PDF `/Info /CreationDate` + `/ModDate` 字节扫描。
//!
//! PDF Date 格式（ISO 32000-1 § 7.9.4）：`(D:YYYYMMDDHHmmSSOHH'mm')`，O 是 `+`/`-`/`Z`，
//! 秒/时区可省略，最短 `D:YYYYMMDD` 仅日期。头 64 KB + 尾 64 KB 双窗口扫描 key 后
//! 第一个 `(D:...)` 字面量——线性化 PDF 的 `/Info` 在头部，未线性化大 PDF 的
//! trailer/`/Info` 在文件尾；`/Info` 落在中段（xref 指向文件中部对象）时仍漏检
//! fallback 到 P4 mtime（YAGNI 不解析 xref）。`MediaReader: Read + Seek` 恒成立
//! （Local mmap / Remote 内存 Cursor），seek 廉价无需不可 seek 分支。
//!
//! 拆分为三文件（原 327 行 → ≤300）：
//! - `dates`（`pdf_dates.rs`）：日期解析族（`extract_dates` / `parse_pdf_d_format` / …）
//! - `text`（`pdf_text.rs`）：文本提取族（`extract_text_from_buf` / `collect_string_literals` / …）
//! - 本文件：parse 入口 + 窗口划分 + 共享 `find_subslice`

use std::io::{Read, SeekFrom};
use std::ops::Range;

use crate::entities::backend::MediaReader;

#[path = "pdf_dates.rs"]
mod dates;
#[path = "pdf_text.rs"]
mod text;

pub(super) use dates::extract_dates;
#[cfg(test)]
pub(super) use dates::parse_pdf_d_format;
#[doc(hidden)]
pub use text::collect_string_literals;
#[doc(hidden)]
pub use text::extract_text_from_buf;

// 上述四者为 office 公开面（`extract_dates` / `extract_text_from_buf` 生产消费，
// 另两个仅 pdf_tests.rs 经 `use super::*` 引用）。以下私有 helper 同样仅测试构建
// 导入，避免生产 unused_imports；子模块内声明为 `pub(super)`，作用域仍限 pdf 内。
#[cfg(test)]
use dates::{parse_pair, parse_tz_offset, scan_d_date_after_key};

/// PDF 字节扫描单窗口大小；头尾各一个窗口。
const PDF_SCAN_BYTES: u64 = 64 * 1024;

/// 入口：按 [`scan_windows`] 读头（+尾）窗口拼 buffer 后委托 `extract_dates`。
/// 头窗口字节在 buffer 前部，`find_subslice` 取首个匹配天然实现「头优先」；
/// 两窗口不相交，拼接只可能边界撕裂漏检（与旧单窗口同类可接受限制）不会假阳性。
///
/// 整 fn `coverage(off)`：seek/read Err arm 只能用 lib unit `FailRead` 注入触发，
/// subprocess bin instance 永远走 OK arm，多 instance 累加 phantom miss。
/// 窗口划分由纯 fn `scan_windows` 单测真测。
#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, _mime: &str) -> (u64, u64) {
    let Some(buf) = read_windows(reader) else {
        return (0, 0);
    };
    extract_dates(&buf)
}

/// 纯窗口划分：`size ≤ 2×64 KB` 整读单窗口（无重叠重复解析）；更大文件头尾各
/// 64 KB 两段不相交窗口。
#[doc(hidden)]
#[must_use]
pub fn scan_windows(size: u64) -> (Range<u64>, Option<Range<u64>>) {
    if size <= 2 * PDF_SCAN_BYTES {
        (0..size, None)
    } else {
        (0..PDF_SCAN_BYTES, Some(size - PDF_SCAN_BYTES..size))
    }
}

/// IO 胶水：seek End 取 size → 按窗口读拼 buffer。`coverage(off)` 理由同 `parse`。
fn read_windows(reader: &mut dyn MediaReader) -> Option<Vec<u8>> {
    let size = reader.seek(SeekFrom::End(0)).ok()?;
    let (head, tail) = scan_windows(size);
    reader.seek(SeekFrom::Start(head.start)).ok()?;
    let head_len = usize::try_from(head.end - head.start).ok()?;
    let mut buf = Vec::with_capacity(head_len);
    reader
        .take(head.end - head.start)
        .read_to_end(&mut buf)
        .ok()?;
    if let Some(tail) = tail {
        reader.seek(SeekFrom::Start(tail.start)).ok()?;
        reader
            .take(tail.end - tail.start)
            .read_to_end(&mut buf)
            .ok()?;
    }
    Some(buf)
}

/// 文本提取的整文件读取上限：正文 content stream 分布全文件，4 MiB 覆盖
/// 常见文档前几十页（远端 backend 已整文件入堆，本 cap 只防超大 PDF 吃内存）。
const PDF_TEXT_INPUT_CAP: usize = 4 * 1024 * 1024;

/// 文本提取入口：扫 `stream…endstream` 块，`/FlateDecode` 的 flate2 解压后抓
/// 文本运算符的 `(...)` 字符串字面量。CID 字体（中文常见）输出 hex 串提不出
/// 可读文本、加密/扫描版无文本层——返空走 `uncategorized`（已知限制）。
///
/// 整 fn `coverage(off)`：read 入口 Err arm 同 `parse`；扫描业务由
/// `extract_text_from_buf` 单测真测。
#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let mut buf = Vec::new();
    let mut limited = reader.take(PDF_TEXT_INPUT_CAP as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    extract_text_from_buf(&buf, max_bytes)
}

/// `slice::iter().position(closure)` 的非 closure 等价物 —— closure region 在多
/// codegen instance 下易出 phantom miss（CLAUDE.md「closure 算独立 function」）；
/// 手写 for 循环让 LLVM 把整 fn 算单 region 累加到两 instance。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
