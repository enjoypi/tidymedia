use std::str::FromStr;

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

const SCHEME_LOCAL: &str = "local";
const SCHEME_SMB: &str = "smb";
const SEP: &str = "://";
#[path = "uri_parse.rs"]
mod uri_parse;

use uri_parse::{decode, render_smb, split_first_segment, split_host_port, split_user};

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
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("missing host in {0:?}")]
    MissingHost(String),
    #[error("missing share in {0:?}")]
    MissingShare(String),
    #[error("invalid percent-encoding in {0:?}")]
    PercentDecode(String),
    #[error("unsupported scheme: {0:?}")]
    UnsupportedScheme(String),
    #[error("invalid port: {0:?}")]
    InvalidPort(String),
}

impl Location {
    #[expect(clippy::missing_errors_doc)]
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let Some((scheme, rest)) = s.split_once(SEP) else {
            return Ok(Self::Local(Utf8PathBuf::from(s)));
        };
        let scheme_lower = scheme.to_ascii_lowercase();
        match scheme_lower.as_str() {
            SCHEME_LOCAL => Ok(Self::Local(Utf8PathBuf::from(decode(rest)?))),
            SCHEME_SMB => Self::parse_smb(rest),
            _ => Err(ParseError::UnsupportedScheme(scheme.to_string())),
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

    #[must_use]
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Local(_) => SCHEME_LOCAL,
            Self::Smb { .. } => SCHEME_SMB,
        }
    }

    #[must_use]
    pub fn path(&self) -> &Utf8Path {
        match self {
            Self::Local(path) | Self::Smb { path, .. } => path.as_path(),
        }
    }

    #[must_use]
    pub fn join_path(&self, segment: &str) -> Self {
        match self {
            Self::Local(path) => Self::Local(path.join(segment)),
            Self::Smb { .. } => {
                let base = self.path().as_str();
                let combined = if base.is_empty() {
                    segment.to_string()
                } else if base.ends_with('/') {
                    format!("{base}{segment}")
                } else {
                    format!("{base}/{segment}")
                };
                self.with_path(Utf8PathBuf::from(combined))
            }
        }
    }

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
        }
    }

    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Local(path) => path.to_string(),
            Self::Smb {
                user,
                host,
                port,
                share,
                path,
            } => render_smb(user.as_deref(), host, *port, share, path.as_str()),
        }
    }
}

impl FromStr for Location {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
#[path = "uri_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "uri_display_tests.rs"]
mod display_tests;
