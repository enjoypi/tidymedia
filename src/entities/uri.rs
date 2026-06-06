//! URI 解析：把 CLI 字符串映射为 [`Location`]，区分本地 / SMB / MTP / ADB。
//!
//! 语法：
//! - 无 `://` 或 `local://` ⇒ 本地路径
//! - `smb://[user@]host[:port]/share[/path]`
//! - `mtp://device/storage[/path]`
//! - `adb://[serial]/abs/path` —— Android 设备走 adb 协议；serial 为空时表示让
//!   `adb_client` 自动选唯一在线设备；path 始终是设备上的绝对路径（`/sdcard/...`）
//!
//! 字段内的空格 / 中文 / 路径分隔符走 percent-encoding。
//! 详细约定见 CLAUDE.md「URI 与 Backend」段。

use std::str::FromStr;

use camino::{Utf8Path, Utf8PathBuf};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use thiserror::Error;

const SCHEME_LOCAL: &str = "local";
const SCHEME_SMB: &str = "smb";
const SCHEME_MTP: &str = "mtp";
const SCHEME_ADB: &str = "adb";
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
    Adb {
        /// 设备 serial（`adb devices` 列出的标识）；`None` 表示由 client 自动选择
        /// 唯一在线设备，对应 URI `adb:///path` 形态
        serial: Option<String>,
        /// 设备上的绝对路径，始终以 `/` 开头（如 `/sdcard/DCIM`）
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
    #[error("missing path in {0:?}")]
    MissingPath(String),
    #[error("invalid percent-encoding in {0:?}")]
    PercentDecode(String),
    #[error("unsupported scheme: {0:?}")]
    UnsupportedScheme(String),
    #[error("invalid port: {0:?}")]
    InvalidPort(String),
}

impl Location {
    /// 将字符串解析为 `Location`，自动识别 scheme（本地 / SMB / MTP / ADB）。
    ///
    /// # Errors
    ///
    /// 当 URI 格式不合法（缺少 host、share、path，percent-encoding 错误，端口非数字，
    /// 或使用了不支持的 scheme）时返回 [`ParseError`]。
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let Some((scheme, rest)) = s.split_once(SEP) else {
            return Ok(Self::Local(Utf8PathBuf::from(s)));
        };
        match scheme {
            SCHEME_LOCAL => Ok(Self::Local(Utf8PathBuf::from(decode(rest)?))),
            SCHEME_SMB => Self::parse_smb(rest),
            SCHEME_MTP => Self::parse_mtp(rest),
            SCHEME_ADB => Self::parse_adb(rest),
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

    fn parse_adb(rest: &str) -> Result<Self, ParseError> {
        // rest 形态：
        //   "EMULATOR5554/sdcard/DCIM" → serial=Some, path=/sdcard/DCIM
        //   "/sdcard/DCIM"             → serial=None, path=/sdcard/DCIM
        //   "EMULATOR5554"             → 缺 path（adb 没有 share/storage 抽象，path 必填）
        //   ""                         → 缺 path（adb:/// 无内容）
        let (serial_raw, tail) = rest
            .split_once('/')
            .ok_or_else(|| ParseError::MissingPath(rest.to_string()))?;
        let serial = if serial_raw.is_empty() {
            None
        } else {
            Some(decode(serial_raw)?)
        };
        let decoded_tail = decode_path(tail)?;
        if decoded_tail.is_empty() {
            return Err(ParseError::MissingPath(rest.to_string()));
        }
        // 设备上始终是绝对路径；split_once('/') 后 tail 不再带前导 '/'
        let mut abs = String::with_capacity(decoded_tail.len() + 1);
        abs.push('/');
        abs.push_str(&decoded_tail);
        Ok(Self::Adb {
            serial,
            path: Utf8PathBuf::from(abs),
        })
    }

    #[must_use]
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Local(_) => SCHEME_LOCAL,
            Self::Smb { .. } => SCHEME_SMB,
            Self::Mtp { .. } => SCHEME_MTP,
            Self::Adb { .. } => SCHEME_ADB,
        }
    }

    /// 返回内部 path 字段（所有 variant 都持 path：Local 是绝对路径，
    /// Smb 是 share 内相对路径，Mtp 是 storage 内相对路径，Adb 是设备上绝对路径）。
    #[must_use]
    pub fn path(&self) -> &Utf8Path {
        match self {
            Self::Local(p) => p.as_path(),
            Self::Smb { path, .. } | Self::Mtp { path, .. } | Self::Adb { path, .. } => {
                path.as_path()
            }
        }
    }

    /// 保留 scheme + 连接参数（user/host/share / device/storage / serial），
    /// 覆写 path 字段。用于在远端 backend 下 join 子目录
    /// （如 `output.with_path(year/month/file)`）。
    #[must_use]
    pub fn with_path(&self, new_path: Utf8PathBuf) -> Self {
        match self {
            Self::Local(_) => Self::Local(new_path),
            Self::Smb {
                user,
                host,
                port,
                share,
                ..
            } => Self::Smb {
                user: user.clone(),
                host: host.clone(),
                port: *port,
                share: share.clone(),
                path: new_path,
            },
            Self::Mtp {
                device, storage, ..
            } => Self::Mtp {
                device: device.clone(),
                storage: storage.clone(),
                path: new_path,
            },
            Self::Adb { serial, .. } => Self::Adb {
                serial: serial.clone(),
                path: new_path,
            },
        }
    }

    #[must_use]
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
            Self::Adb { serial, path } => render_adb(serial.as_deref(), path.as_str()),
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
        .map(std::borrow::Cow::into_owned)
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
    s.split('/').map(encode).collect::<Vec<_>>().join("/")
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

fn render_adb(serial: Option<&str>, path: &str) -> String {
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

#[cfg(test)]
#[path = "uri_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "uri_display_tests.rs"]
mod display_tests;
