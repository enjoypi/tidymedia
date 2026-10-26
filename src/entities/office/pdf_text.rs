use super::super::flate_io::inflate_capped;
use super::super::scan::truncate_at_boundary;
use super::find_subslice;

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
