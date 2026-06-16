//! JPEG/HEIC/TIFF APP1 XMP packet 单段自解析（fallback）。
//!
//! 用途：相机原拍后用 Lightroom/Bridge/ACR 等 re-tag 工具改过的图，EXIF IFD0
//! 可能只剩 `ModifyDate`、原始拍摄时间落在 XMP packet 的 `xmp:CreateDate` /
//! `photoshop:DateCreated`。nom-exif 3.5/3.6 只读 EXIF IFD0，遇到这类文件
//! 会跳过日期走 P4 `FsMtime` 兜底归错桶。本模块在 `entities::exif` 解析后做
//! 兜底嗅探，与 exiftool 行为对齐。
//!
//! 范围：纯函数 + 单段 APP1 packet（≤ 64 KB 窗口足够），不引 XML lib。
//! 已知不支持：ExtendedXMP（跨多 APP1 段拼接，标识符
//! `http://ns.adobe.com/xmp/extension/\0`）；element 形式
//! `<xmp:CreateDate>...</xmp:CreateDate>`（Adobe 系工具默认写 attribute）。

use chrono::DateTime;
use chrono::FixedOffset;

const PACKET_START: &str = "<x:xmpmeta";
const PACKET_END: &str = "</x:xmpmeta>";
// XML attribute 值可用单引号或双引号包裹（W3C XML 1.0 §3.1）；XMP packet 在
// 不同工具下两种都见过（exiftool Shorthand 输出单引号、Adobe 系工具多用双引号）。
const KEY_PHOTOSHOP_DATE_CREATED: &str = "photoshop:DateCreated=";
const KEY_XMP_CREATE_DATE: &str = "xmp:CreateDate=";

/// XMP packet 内的两个候选时间。两键互独立，可能同时为 Some/None。
#[doc(hidden)]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct XmpDates {
    /// `photoshop:DateCreated`：等价 EXIF `DateTimeOriginal`（P0）。
    pub photoshop_date_created: Option<DateTime<FixedOffset>>,
    /// `xmp:CreateDate`：等价 EXIF `CreateDate`（P1）。
    pub xmp_create_date: Option<DateTime<FixedOffset>>,
}

/// 在原始字节流中线性搜 `<x:xmpmeta ... </x:xmpmeta>` 子串。
/// `buf` 通常是 JPEG/HEIC/TIFF 前 ~64 KB；含非 UTF-8 字节也返回 None
/// （`from_utf8` 验证一次性，避免后续解析触雷）。
pub(crate) fn find_xmp_packet(buf: &[u8]) -> Option<&str> {
    let start_pat = PACKET_START.as_bytes();
    let end_pat = PACKET_END.as_bytes();
    let start = buf.windows(start_pat.len()).position(|w| w == start_pat)?;
    // start 是 windows().position 返回的合法索引；tail 切片不会越界。
    let tail = &buf[start..];
    let end_rel = tail.windows(end_pat.len()).position(|w| w == end_pat)?;
    // end_rel + end_pat.len() ≤ tail.len() 由 windows 语义保证。
    let packet = &tail[..end_rel + end_pat.len()];
    std::str::from_utf8(packet).ok()
}

/// 解析 XMP packet 子串，返回 photoshop:DateCreated 与 xmp:CreateDate 两键
/// 的 attribute 值（RFC3339）。仅 attribute 形态；element 形态 YAGNI。
#[doc(hidden)]
#[must_use]
pub fn parse_xmp_dates(content: &str) -> XmpDates {
    let stripped = strip_xml_comments(content);
    XmpDates {
        photoshop_date_created: find_attr_rfc3339(&stripped, KEY_PHOTOSHOP_DATE_CREATED),
        xmp_create_date: find_attr_rfc3339(&stripped, KEY_XMP_CREATE_DATE),
    }
}

fn find_attr_rfc3339(haystack: &str, key: &str) -> Option<DateTime<FixedOffset>> {
    // XML attribute 边界：key 必须紧跟 whitespace 或 packet/element 起始（'<'），
    // 否则 `dc:description="...xmp:CreateDate='OLD'..."` 这类属性值内子串会
    // 抢在真实属性前命中。线性扫描多次 find，匹配到合法 RFC3339 即返；
    // 边界通过但 parse 失败（如 description 含字面 `photoshop:DateCreated="text"`
    // 注释残留）必须 continue 而非 return None，否则真实属性在后面出现将被永久跳过。
    let bytes = haystack.as_bytes();
    let key_bytes = key.as_bytes();
    let mut search_from = 0usize;
    loop {
        // search_from 不变量：每轮推进 = key_idx + key_bytes.len() ≤ haystack.len()，
        // 故 `&haystack[search_from..]` 永不越界（直接切片省一处不可达的 `?` 死区）。
        let rel = haystack[search_from..].find(key)?;
        let key_idx = search_from + rel;
        // 推进 cursor 前置：所有 continue 路径共享同一更新无遗漏死循环。
        // find 已确保 key_idx + key_bytes.len() ≤ haystack.len()，切片不越界。
        search_from = key_idx + key_bytes.len();
        let prev = if key_idx == 0 {
            b' '
        } else {
            bytes[key_idx - 1]
        };
        // 边界字符 = ASCII 单字节即可：whitespace（space/tab/LF/CR）或 '<' 起始
        // attribute（针对 element 形态 <ns:Key>... 不命中，YAGNI 保持单段属性）。
        if !matches!(prev, b' ' | b'\t' | b'\n' | b'\r' | b'<') {
            continue;
        }
        let after_eq = &haystack[search_from..];
        let Some(quote) = after_eq.chars().next() else {
            continue;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let rest = &after_eq[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            continue;
        };
        if let Ok(dt) = DateTime::parse_from_rfc3339(&rest[..end]) {
            return Some(dt);
        }
    }
}

// 把 `<!-- ... -->` 注释体替换为同字节数的空格，保持偏移与原串一致。
// XML 注释不可嵌套，按 `<!--`/`-->` 线性配对。注释边界全 ASCII，操作不破坏 UTF-8。
// 未闭合 `<!--` 视为延伸到 EOF 的注释体（与 lenient XML 解析对齐）：避免把注释
// 中的 `photoshop:DateCreated="OLD"` 字面量误读为合法属性。XMP packet 是 well-formed
// XML 子集，未闭合注释属 malformed，宁可放弃后续也不冒误读风险。
pub(crate) fn strip_xml_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<!--") {
            let body_start = i + 4;
            let end = bytes
                .get(body_start..)
                .and_then(|tail| tail.windows(3).position(|w| w == b"-->"))
                .map_or(bytes.len(), |p| body_start + p + 3);
            for _ in i..end {
                out.push(' ');
            }
            i = end;
        } else {
            // 非注释字节按 UTF-8 字符边界推进，避免切到多字节字符内部。
            let ch_end = (i + 1..=bytes.len())
                .find(|&j| content.is_char_boundary(j))
                .unwrap_or(bytes.len());
            out.push_str(&content[i..ch_end]);
            i = ch_end;
        }
    }
    out
}

#[cfg(test)]
#[path = "xmp_tests.rs"]
mod tests;
