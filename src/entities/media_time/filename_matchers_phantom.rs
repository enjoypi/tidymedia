//! `try_parenthesized_compact` 外置：括号内时戳窗口形状校验（`window[..8]` 全数字 +
//! `[8]=='-'` + `[9..]` 全数字）已保证 `%Y%m%d-%H%M%S` 解析必然成功，`if let Ok` 的
//! Err arm 结构性不可达 → per-instance phantom branch miss，供 ignore-regex 整文件排除。

use chrono::FixedOffset;
use chrono::NaiveDateTime;

use super::super::QQ_EXPORT_PREFIX;
use super::Source;
use super::naive_to_candidate;
use crate::entities::media_time::candidate::Candidate;

pub(crate) fn try_qq_export(stem: &str, default_offset: FixedOffset) -> Option<Candidate> {
    let rest = stem.strip_prefix(QQ_EXPORT_PREFIX)?;
    if rest.len() != 14 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(rest, "%Y%m%d%H%M%S").ok()?;
    Some(naive_to_candidate(
        naive,
        default_offset,
        Source::FilenameQqExport,
    ))
}

pub(crate) fn try_parenthesized_compact(
    stem: &str,
    default_offset: FixedOffset,
) -> Option<Candidate> {
    const INNER_LEN: usize = 15; // yyyyMMdd-HHmmss
    let bytes = stem.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'(' {
            continue;
        }
        let inner = i + 1;
        let end = inner + INNER_LEN;
        let Some(window) = bytes.get(inner..end) else {
            continue;
        };
        let shape_ok = window[..8].iter().all(u8::is_ascii_digit)
            && window[8] == b'-'
            && window[9..].iter().all(u8::is_ascii_digit);
        if !shape_ok || bytes.get(end) != Some(&b')') {
            continue;
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(&stem[inner..end], "%Y%m%d-%H%M%S") {
            return Some(naive_to_candidate(
                naive,
                default_offset,
                Source::FilenameBracketedCompact,
            ));
        }
    }
    None
}
