//! office 文本提取共用纯 helper：XML/HTML 剥标签、char boundary 截断。
//! 与 `entities::tiff_ifd`（riff/exif 共用）同定位——纯函数全分支单测保覆盖率。

/// 把 `buf` 中标签外文本追加进 `out`（连续空白折叠单空格），
/// 累计长度达 `max_bytes` 即停。XML 实体（`&amp;` 等）原样保留——
/// 分类 embedding 对少量实体噪声不敏感（YAGNI 不解码）。
#[doc(hidden)]
pub fn strip_markup_into(buf: &[u8], out: &mut String, max_bytes: usize) {
    let budget = max_bytes.saturating_sub(out.len());
    if budget == 0 {
        return;
    }
    let mut text: Vec<u8> = Vec::new();
    let mut in_tag = false;
    let mut last_space = true;
    for &b in buf {
        if text.len() >= budget {
            break;
        }
        if b == b'<' {
            in_tag = true;
            if !last_space {
                text.push(b' ');
                last_space = true;
            }
        } else if in_tag {
            if b == b'>' {
                in_tag = false;
            }
        } else if b.is_ascii_whitespace() {
            if !last_space {
                text.push(b' ');
                last_space = true;
            }
        } else {
            text.push(b);
            last_space = false;
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

/// 截断 `s` 到不超过 `max_bytes` 且落在 char boundary 上（防 panic）。
#[doc(hidden)]
pub fn truncate_at_boundary(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut n = max_bytes;
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    s.truncate(n);
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
