//! PDF 文本层提取族：扫 `stream…endstream` 块，`/FlateDecode` 解压后收集
//! `BT…ET` 内 `(...)` 字符串字面量。`extract_text_from_buf` 是纯字节扫描业务；
//! IO 读取入口 `extract_text` 在父模块；共享 `find_subslice` 在父模块。

use std::io::Read;

use super::super::scan::truncate_at_boundary;
use super::find_subslice;

/// 单 content stream 解压输出上限：防 Flate 炸弹。
const INFLATE_OUTPUT_CAP: usize = 2 * 1024 * 1024;

/// 纯字节扫描业务：遍历 `stream`/`endstream` 对，按前导字典是否含
/// `/FlateDecode` 决定解压，再收集 `BT…ET` 文本块内的字符串字面量。
#[must_use]
pub fn extract_text_from_buf(buf: &[u8], max_bytes: usize) -> String {
    let mut out = String::new();
    let mut pos = 0;
    while out.len() < max_bytes {
        let Some(rel) = find_subslice(&buf[pos..], b"stream") else {
            break;
        };
        let kw_start = pos + rel;
        let data_start = skip_stream_eol(buf, kw_start + b"stream".len());
        let Some(end_rel) = find_subslice(&buf[data_start..], b"endstream") else {
            break;
        };
        let data = &buf[data_start..data_start + end_rel];
        // 字典在 `stream` 关键字前的 `<<…>>`；仅回看 1 KiB（Length/Filter 字典很短）。
        let dict_window_start = kw_start.saturating_sub(1024);
        let dict = &buf[dict_window_start..kw_start];
        if find_subslice(dict, b"/FlateDecode").is_some() {
            if let Some(inflated) = inflate_capped(data) {
                collect_string_literals(&inflated, &mut out, max_bytes);
            }
        } else {
            collect_string_literals(data, &mut out, max_bytes);
        }
        pos = data_start + end_rel + b"endstream".len();
    }
    out
}

/// zlib 解压 content stream，输出 cap 防炸弹；非 zlib 数据（加密/其它 filter
/// 链）返 `None` 跳过。
pub(super) fn inflate_capped(data: &[u8]) -> Option<Vec<u8>> {
    let decoder = flate2::read::ZlibDecoder::new(data);
    let mut inflated = Vec::new();
    let n = decoder
        .take(INFLATE_OUTPUT_CAP as u64)
        .read_to_end(&mut inflated);
    match n {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(inflated),
    }
}

/// 收集 content 内 `(...)` 字符串字面量（PDF 文本 show 运算符 `Tj`/`TJ`/`'`/`"`
/// 的操作数），处理 `\(` `\)` `\\` 转义与嵌套括号；仅当 content 含 `BT`
/// （text block 开始运算符）才扫，过滤纯图形 stream。
pub fn collect_string_literals(content: &[u8], out: &mut String, max_bytes: usize) {
    if find_subslice(content, b"BT").is_none() {
        return;
    }
    let mut text: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < content.len() && out.len() + text.len() < max_bytes {
        if content[i] != b'(' {
            i += 1;
            continue;
        }
        i += 1;
        let mut depth = 1;
        let mut got_any = false;
        while i < content.len() && depth > 0 {
            match content[i] {
                b'\\' if i + 1 < content.len() => {
                    let esc = content[i + 1];
                    if matches!(esc, b'(' | b')' | b'\\') {
                        text.push(esc);
                        got_any = true;
                    }
                    i += 2;
                    continue;
                }
                b'(' => {
                    depth += 1;
                    text.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth > 0 {
                        text.push(b')');
                    }
                }
                c => {
                    text.push(c);
                    got_any = true;
                }
            }
            i += 1;
        }
        if got_any {
            text.push(b' ');
        }
    }
    let lossy = String::from_utf8_lossy(&text);
    let trimmed = lossy.trim_end();
    if !trimmed.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(trimmed);
    }
    truncate_at_boundary(out, max_bytes);
}

/// `stream` 关键字后跳过 EOL（`\r\n` 或 `\n`，PDF spec §7.3.8）。
pub(super) fn skip_stream_eol(buf: &[u8], mut i: usize) -> usize {
    if buf.get(i) == Some(&b'\r') {
        i += 1;
    }
    if buf.get(i) == Some(&b'\n') {
        i += 1;
    }
    i
}

#[cfg(test)]
#[path = "pdf_text_tests.rs"]
mod tests;
