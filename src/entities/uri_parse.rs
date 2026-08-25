//! URI 解析/渲染 helper：percent-encoding 编解码、user/host/port/share 拆分、
//! 各远端 scheme 的字符串渲染。内部模块，仅被 `uri` 主模块消费。

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

use super::{ParseError, SCHEME_ADB, SCHEME_MTP, SCHEME_SMB, SEP};

pub(super) fn split_user(auth: &str) -> Result<(Option<String>, &str), ParseError> {
    match auth.split_once('@') {
        Some((u, h)) => Ok((Some(decode(u)?), h)),
        None => Ok((None, auth)),
    }
}

pub(super) fn split_host_port<'a>(
    hostport: &'a str,
    rest: &str,
) -> Result<(&'a str, Option<u16>), ParseError> {
    // IPv6 字面量带方括号：`[2001:db8::1]` 或 `[::1]:445`。方括号内冒号属于地址，
    // 不能用 `split_once(':')` 朴素拆。先识别 `[…]` 包裹段，再判可选 `:port`。
    if let Some(rest_after_bracket) = hostport.strip_prefix('[') {
        let Some(end) = rest_after_bracket.find(']') else {
            return Err(ParseError::InvalidPort(format!("{rest}@{hostport}")));
        };
        let host = &hostport[..=end + 1]; // 含两端方括号
        let tail = &rest_after_bracket[end + 1..];
        if tail.is_empty() {
            return Ok((host, None));
        }
        let Some(port_str) = tail.strip_prefix(':') else {
            return Err(ParseError::InvalidPort(format!("{rest}@{tail}")));
        };
        let port = port_str
            .parse::<u16>()
            .map_err(|_| ParseError::InvalidPort(format!("{rest}@{port_str}")))?;
        return Ok((host, Some(port)));
    }
    match hostport.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| ParseError::InvalidPort(format!("{rest}@{p}")))?;
            Ok((h, Some(port)))
        }
        None => Ok((hostport, None)),
    }
}

pub(super) fn split_first_segment(tail: &str) -> Result<(String, String), ParseError> {
    match tail.split_once('/') {
        Some((first, rest)) => {
            let first_decoded = decode(first)?;
            let rest_decoded = decode_path(rest)?;
            Ok((first_decoded, rest_decoded))
        }
        None => Ok((decode(tail)?, String::new())),
    }
}

pub(super) fn decode(s: &str) -> Result<String, ParseError> {
    percent_decode_str(s)
        .decode_utf8()
        .map(std::borrow::Cow::into_owned)
        .map_err(|_| ParseError::PercentDecode(s.to_string()))
}

pub(super) fn decode_path(s: &str) -> Result<String, ParseError> {
    let mut out = String::with_capacity(s.len());
    for (idx, seg) in s.split('/').enumerate() {
        if idx > 0 {
            out.push('/');
        }
        out.push_str(&decode(seg)?);
    }
    Ok(out)
}

const URI_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'#')
    .add(b'?')
    .add(b'@')
    .add(b':')
    .add(b'/');

fn encode(s: &str) -> String {
    utf8_percent_encode(s, URI_ENCODE).to_string()
}

fn encode_path(s: &str) -> String {
    s.split('/').map(encode).collect::<Vec<_>>().join("/")
}

pub(super) fn render_smb(
    user: Option<&str>,
    host: &str,
    port: Option<u16>,
    share: &str,
    path: &str,
) -> String {
    let mut out = format!("{SCHEME_SMB}{SEP}");
    if let Some(u) = user {
        out.push_str(&encode(u));
        out.push('@');
    }
    out.push_str(host);
    if let Some(p) = port {
        out.push(':');
        out.push_str(&p.to_string());
    }
    out.push('/');
    out.push_str(&encode(share));
    if !path.is_empty() {
        out.push('/');
        out.push_str(&encode_path(path));
    }
    out
}

pub(super) fn render_mtp(device: &str, storage: &str, path: &str) -> String {
    let mut out = format!("{SCHEME_MTP}{SEP}");
    out.push_str(&encode(device));
    out.push('/');
    out.push_str(&encode(storage));
    if !path.is_empty() {
        out.push('/');
        out.push_str(&encode_path(path));
    }
    out
}

pub(super) fn render_adb(serial: Option<&str>, path: &str) -> String {
    let mut out = format!("{SCHEME_ADB}{SEP}");
    if let Some(s) = serial {
        out.push_str(&encode(s));
    }
    // path 已是 `/abs`，直接编码各段后拼接；前导 '/' 让 `adb:///abs` 形态自然出现
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    out.push('/');
    out.push_str(&encode_path(trimmed));
    out
}
