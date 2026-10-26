use std::io;
use std::time::Duration;

use camino::Utf8PathBuf;
use parking_lot::Mutex;

use super::super::remote::RemoteClient;
use super::{SmbTarget, entries_from_listing, map_smb_err, metadata_from_file_info, path_or_root};
use crate::entities::backend::{Entry, Metadata};

pub struct RealSmbClient {
    runtime: tokio::runtime::Runtime,
    inner: Mutex<ShareSession>,
    conn: SmbTarget,
}

struct ShareSession {
    client: smb2::SmbClient,
    tree: smb2::Tree,
}

impl std::fmt::Debug for RealSmbClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealSmbClient")
            .field("host", &self.conn.host)
            .field("share", &self.conn.share)
            .field("user", &self.conn.user)
            .finish_non_exhaustive()
    }
}

impl RealSmbClient {
    pub fn new(
        target: &SmbTarget,
        default_user: &str,
        workgroup: &str,
        timeout_secs: u64,
    ) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(io::Error::other)?;
        let user = target
            .user
            .clone()
            .unwrap_or_else(|| default_user.to_owned());
        let config = smb2::ClientConfig {
            addr: format!("{}:{}", target.host, target.port.unwrap_or(445)),
            timeout: Duration::from_secs(timeout_secs),
            username: user,
            password: target.password.clone().unwrap_or_default(),
            domain: workgroup.to_owned(),
            auto_reconnect: false,
            compression: false,
            dfs_enabled: true,
            dfs_target_overrides: std::collections::HashMap::new(),
        };
        let (client, tree) = runtime
            .block_on(async {
                let mut client = smb2::SmbClient::connect(config).await?;
                let tree = client.connect_share(&target.share).await?;
                Ok::<_, smb2::Error>((client, tree))
            })
            .map_err(map_smb_err)?;
        Ok(Self {
            runtime,
            inner: Mutex::new(ShareSession { client, tree }),
            conn: SmbTarget {
                user: target.user.clone(),
                host: target.host.clone(),
                port: target.port,
                share: target.share.clone(),
                path: Utf8PathBuf::default(),
                password: None,
                krb5_ccname: None,
            },
        })
    }
}

impl RemoteClient<SmbTarget> for RealSmbClient {
    fn stat(&self, target: &SmbTarget) -> io::Result<Metadata> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.stat(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
            .map(|info| metadata_from_file_info(&info))
    }

    fn list(&self, target: &SmbTarget) -> io::Result<Vec<Entry>> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.list_directory(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
            .map(|entries| entries_from_listing(&self.conn, target, entries))
    }

    fn read(&self, target: &SmbTarget) -> io::Result<Vec<u8>> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.read_file(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
    }

    fn write(&self, target: &SmbTarget, data: &[u8]) -> io::Result<u64> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.write_file(tree, path_or_root(&target.path), data))
            .map_err(map_smb_err)
    }

    fn unlink(&self, target: &SmbTarget) -> io::Result<()> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.delete_file(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
    }

    fn mkdir(&self, target: &SmbTarget) -> io::Result<()> {
        let mut guard = self.inner.lock();
        let ShareSession { client, tree } = &mut *guard;
        self.runtime
            .block_on(client.create_directory(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)
    }
}
