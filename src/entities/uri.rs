//! URI 解析：把 CLI 字符串映射为 [`Location`]，区分本地 / SMB / MTP。
//!
//! 语法：
//! - 无 `://` 或 `local://` ⇒ 本地路径
//! - `smb://[user@]host[:port]/share[/path]`
//! - `mtp://device/storage[/path]`
//!
//! 字段内的空格 / 中文 / 路径分隔符走 percent-encoding。
//! 详细约定见 CLAUDE.md「URI 与 Backend」段。

use std::str::FromStr;

use camino::Utf8PathBuf;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use thiserror::Error;

const SCHEME_LOCAL: &str = "local";
const SCHEME_SMB: &str = "smb";
const SCHEME_MTP: &str = "mtp";
const SEP: &str = "://";

/// 业务对象：定位一段媒体内容所在的存储位置。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Location {
    Local(Utf8PathBuf),
    Smb {
        user: Option<String>,
        host: String,
        port: Option<u16>,
        share: String,
        path: Utf8PathBuf,
    },
    Mtp {
        device: String,
        storage: String,
        path: Utf8PathBuf,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("missing host in {0:?}")]
    MissingHost(String),
    #[error("missing share in {0:?}")]
    MissingShare(String),
    #[error("missing storage in {0:?}")]
    MissingStorage(String),
    #[error("invalid percent-encoding in {0:?}")]
    PercentDecode(String),
    #[error("unsupported scheme: {0:?}")]
    UnsupportedScheme(String),
    #[error("invalid port: {0:?}")]
    InvalidPort(String),
}

impl Location {
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let Some((scheme, rest)) = s.split_once(SEP) else {
            return Ok(Self::Local(Utf8PathBuf::from(s)));
        };
        match scheme {
            SCHEME_LOCAL => Ok(Self::Local(Utf8PathBuf::from(decode(rest)?))),
            SCHEME_SMB => Self::parse_smb(rest),
            SCHEME_MTP => Self::parse_mtp(rest),
            other => Err(ParseError::UnsupportedScheme(other.to_string())),
        }
    }

    fn parse_smb(rest: &str) -> Result<Self, ParseError> {
        let (auth, tail) = rest
            .split_once('/')
            .ok_or_else(|| ParseError::MissingShare(rest.to_string()))?;
        let (user, hostport) = split_user(auth)?;
        let (host, port) = split_host_port(hostport, rest)?;
        if host.is_empty() {
            return Err(ParseError::MissingHost(rest.to_string()));
        }
        let (share, path) = split_first_segment(tail)?;
        if share.is_empty() {
            return Err(ParseError::MissingShare(rest.to_string()));
        }
        Ok(Self::Smb {
            user,
            host: host.to_string(),
            port,
            share,
            path: Utf8PathBuf::from(path),
        })
    }

    fn parse_mtp(rest: &str) -> Result<Self, ParseError> {
        let (device_raw, tail) = rest
            .split_once('/')
            .ok_or_else(|| ParseError::MissingStorage(rest.to_string()))?;
        if device_raw.is_empty() {
            return Err(ParseError::MissingHost(rest.to_string()));
        }
        let device = decode(device_raw)?;
        let (storage, path) = split_first_segment(tail)?;
        if storage.is_empty() {
            return Err(ParseError::MissingStorage(rest.to_string()));
        }
        Ok(Self::Mtp {
            device,
            storage,
            path: Utf8PathBuf::from(path),
        })
    }

    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Local(_) => SCHEME_LOCAL,
            Self::Smb { .. } => SCHEME_SMB,
            Self::Mtp { .. } => SCHEME_MTP,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Self::Local(p) => p.to_string(),
            Self::Smb {
                user,
                host,
                port,
                share,
                path,
            } => render_smb(user.as_deref(), host, *port, share, path.as_str()),
            Self::Mtp {
                device,
                storage,
                path,
            } => render_mtp(device, storage, path.as_str()),
        }
    }
}

impl FromStr for Location {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

fn split_user(auth: &str) -> Result<(Option<String>, &str), ParseError> {
    match auth.split_once('@') {
        Some((u, h)) => Ok((Some(decode(u)?), h)),
        None => Ok((None, auth)),
    }
}

fn split_host_port<'a>(
    hostport: &'a str,
    rest: &str,
) -> Result<(&'a str, Option<u16>), ParseError> {
    match hostport.split_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| ParseError::InvalidPort(format!("{rest}@{p}")))?;
            Ok((h, Some(port)))
        }
        None => Ok((hostport, None)),
    }
}

fn split_first_segment(tail: &str) -> Result<(String, String), ParseError> {
    match tail.split_once('/') {
        Some((first, rest)) => {
            let first_decoded = decode(first)?;
            let rest_decoded = decode_path(rest)?;
            Ok((first_decoded, rest_decoded))
        }
        None => Ok((decode(tail)?, String::new())),
    }
}

fn decode(s: &str) -> Result<String, ParseError> {
    percent_decode_str(s)
        .decode_utf8()
        .map(|c| c.into_owned())
        .map_err(|_| ParseError::PercentDecode(s.to_string()))
}

fn decode_path(s: &str) -> Result<String, ParseError> {
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
    s.split('/')
        .map(encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn render_smb(
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

fn render_mtp(device: &str, storage: &str, path: &str) -> String {
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

#[cfg(test)]
#[path = "uri_tests.rs"]
mod tests;
