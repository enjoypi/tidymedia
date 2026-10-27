use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::remote::{RemoteAdapter, RemoteBackend, RemoteClient, RemoteTarget};
use crate::entities::backend::{Backend, Entry, EntryKind, Metadata};
use crate::entities::uri::Location;

#[derive(Clone, PartialEq, Eq)]
pub struct SmbTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub share: String,
    pub path: Utf8PathBuf,
    pub password: Option<String>,
    pub krb5_ccname: Option<String>,
}

impl std::fmt::Debug for SmbTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmbTarget")
            .field("user", &self.user)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("share", &self.share)
            .field("path", &self.path)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("krb5_ccname", &self.krb5_ccname)
            .finish()
    }
}

impl SmbTarget {
    pub(crate) fn child_target(conn: &SmbTarget, parent: &SmbTarget, name: &str) -> SmbTarget {
        let child_path = if parent.path.as_str().is_empty() {
            Utf8PathBuf::from(name)
        } else {
            let p = parent.path.as_str().trim_end_matches('/');
            Utf8PathBuf::from(format!("{p}/{name}"))
        };
        SmbTarget {
            user: conn.user.clone(),
            host: conn.host.clone(),
            port: conn.port,
            share: conn.share.clone(),
            path: child_path,
            password: parent.password.clone(),
            krb5_ccname: parent.krb5_ccname.clone(),
        }
    }
}

impl RemoteTarget for SmbTarget {
    type Ctx = ();

    fn from_location(loc: &Location, _ctx: &()) -> io::Result<Self> {
        let Location::Smb {
            user,
            host,
            port,
            share,
            path,
        } = loc
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("SmbBackend cannot handle scheme {:?}", loc.scheme()),
            ));
        };
        Ok(SmbTarget {
            user: user.clone(),
            host: host.clone(),
            port: *port,
            share: share.clone(),
            path: path.clone(),
            password: std::env::var("SMB_PASSWORD").ok(),
            krb5_ccname: std::env::var("KRB5CCNAME").ok(),
        })
    }

    fn parent(&self) -> Option<Self> {
        let parent = self.path.parent()?;
        if parent.as_str().is_empty() || parent.as_str() == "/" {
            return None;
        }
        Some(SmbTarget {
            path: parent.to_path_buf(),
            ..self.clone()
        })
    }

    fn entry_location(&self, path: Utf8PathBuf) -> Location {
        Location::Smb {
            user: self.user.clone(),
            host: self.host.clone(),
            port: self.port,
            share: self.share.clone(),
            path,
        }
    }

    fn path(&self) -> &camino::Utf8Path {
        &self.path
    }
}

pub(crate) fn seek_position(current: u64, size: u64, pos: io::SeekFrom) -> io::Result<u64> {
    let next = match pos {
        io::SeekFrom::Start(value) => i128::from(value),
        io::SeekFrom::Current(delta) => i128::from(current) + i128::from(delta),
        io::SeekFrom::End(delta) => i128::from(size) + i128::from(delta),
    };
    u64::try_from(next).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "seek outside the supported file range",
        )
    })
}

pub(crate) fn path_or_root(p: &Utf8PathBuf) -> &str {
    if p.as_str().is_empty() {
        "."
    } else {
        p.as_str()
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "用作 map_err 回调，必须接收 owned smb2::Error"
)]
pub(crate) fn map_smb_err(e: smb2::Error) -> io::Error {
    let kind = match e.kind() {
        smb2::ErrorKind::NotFound => io::ErrorKind::NotFound,
        smb2::ErrorKind::AccessDenied | smb2::ErrorKind::AuthRequired => {
            io::ErrorKind::PermissionDenied
        }
        smb2::ErrorKind::AlreadyExists => io::ErrorKind::AlreadyExists,
        smb2::ErrorKind::TimedOut => io::ErrorKind::TimedOut,
        smb2::ErrorKind::ConnectionLost | smb2::ErrorKind::SessionExpired => {
            io::ErrorKind::ConnectionAborted
        }
        smb2::ErrorKind::DiskFull => io::ErrorKind::StorageFull,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("smb2: {e}"))
}

pub(crate) fn metadata_from_file_info(info: &smb2::FileInfo) -> Metadata {
    Metadata {
        size: info.size,
        kind: if info.is_directory {
            EntryKind::Dir
        } else {
            EntryKind::File
        },
        modified: info.modified.to_system_time(),
        created: info.created.to_system_time(),
    }
}

pub(crate) fn entries_from_listing(
    conn: &SmbTarget,
    parent: &SmbTarget,
    entries: Vec<smb2::DirectoryEntry>,
) -> Vec<Entry> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        if e.name == "." || e.name == ".." {
            continue;
        }
        let kind = if e.is_directory {
            EntryKind::Dir
        } else {
            EntryKind::File
        };
        let child = SmbTarget::child_target(conn, parent, &e.name);
        out.push(Entry {
            location: child.entry_location(child.path.clone()),
            size: e.size,
            kind,
        });
    }
    out
}

pub trait SmbClient: RemoteClient<SmbTarget> {}
impl<T: RemoteClient<SmbTarget>> SmbClient for T {}

pub struct SmbAdapter {
    client: Arc<dyn RemoteClient<SmbTarget>>,
}

impl RemoteAdapter for SmbAdapter {
    type Target = SmbTarget;

    fn scheme() -> &'static str {
        "smb"
    }

    fn map_error(e: io::Error) -> io::Error {
        super::remote::map_remote_error(e)
    }

    fn ctx(&self) -> &() {
        &()
    }

    fn client(&self) -> &Arc<dyn RemoteClient<SmbTarget>> {
        &self.client
    }
}

pub type SmbBackend = RemoteBackend<SmbAdapter>;

impl SmbBackend {
    pub fn with_client(client: Arc<dyn SmbClient>) -> Self {
        let remote: Arc<dyn RemoteClient<SmbTarget>> = client;
        RemoteBackend {
            adapter: SmbAdapter { client: remote },
        }
    }

    pub fn arc_with_client(client: Arc<dyn SmbClient>) -> Arc<dyn Backend> {
        Arc::new(Self::with_client(client))
    }
}

#[cfg(test)]
#[path = "smb_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "smb_internals_tests.rs"]
mod internals_tests;

#[cfg(test)]
#[path = "smb_convert_tests.rs"]
mod convert_tests;

#[path = "smb_real.rs"]
pub mod real;
