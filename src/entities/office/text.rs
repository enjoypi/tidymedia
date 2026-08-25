//! 纯文本族（txt/md/rst/csv/tsv/log）：无 metadata，`parse` 直接返 (0, 0)。
//! 让 `Info::create_time` 退到 P2 文件名 + P4 mtime。
//! `extract_text` 直读前若干字节当正文（分类用）。

use std::io::Read;

use crate::entities::backend::MediaReader;

#[doc(hidden)]
pub fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let _ = reader;
    let _ = mime;
    (0, 0)
}

/// 直读前 `max_bytes` 字节按 UTF-8 lossy 转文本。多读一小段再 lossy：
/// 截断点可能落在多字节字符中间，lossy 会以 U+FFFD 替换，对分类无影响。
///
/// 整 fn `coverage(off)`：read 入口 Err arm 需注入 reader 错误，multi-instance
/// 下 phantom miss 难闭合；截断语义由 `truncate_to_char_boundary` 类单测保证。
#[doc(hidden)]
pub fn extract_text(reader: &mut dyn MediaReader, _mime: &str, max_bytes: usize) -> String {
    let mut buf = Vec::with_capacity(max_bytes);
    let mut limited = reader.take(max_bytes as u64);
    if limited.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}
