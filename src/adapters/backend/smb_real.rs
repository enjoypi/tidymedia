use std::io;
use std::io::{Read, Seek, Write};
use std::sync::Arc;
use std::time::Duration;

use camino::Utf8PathBuf;
use parking_lot::Mutex;

use super::super::remote::RemoteClient;
use super::{
    SmbTarget, entries_from_listing, map_smb_err, metadata_from_file_info, path_or_root,
    seek_position,
};
use crate::entities::backend::{Entry, MediaReader, MediaWriter, Metadata};

struct SmbReader {
    runtime: Arc<tokio::runtime::Runtime>,
    reader: Option<smb2::FileReader>,
    path: String,
    pos: u64,
    size: u64,
}

impl SmbReader {
    fn new(runtime: Arc<tokio::runtime::Runtime>, reader: smb2::FileReader, path: String) -> Self {
        let size = reader.size();
        Self {
            runtime,
            reader: Some(reader),
            path,
            pos: 0,
            size,
        }
    }
}

impl std::fmt::Debug for SmbReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmbReader")
            .field("path", &self.path)
            .field("pos", &self.pos)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Read for SmbReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let Some(reader) = self.reader.as_ref() else {
            return Err(io::Error::other("reader is closed"));
        };
        let data = self
            .runtime
            .block_on(reader.read_at(self.pos, buf.len() as u64))
            .map_err(map_smb_err)?;
        let count = data.len();
        buf[..count].copy_from_slice(&data);
        self.pos += count as u64;
        Ok(count)
    }
}

impl Seek for SmbReader {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.pos = seek_position(self.pos, self.size, pos)?;
        Ok(self.pos)
    }
}

impl Drop for SmbReader {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = self.runtime.block_on(reader.close());
        }
    }
}

struct SmbWriter {
    runtime: Arc<tokio::runtime::Runtime>,
    writer: Option<smb2::FileWriter>,
    path: String,
}

impl std::fmt::Debug for SmbWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmbWriter")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Write for SmbWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let Some(writer) = self.writer.as_mut() else {
            return Err(io::Error::other("writer is closed"));
        };
        self.runtime
            .block_on(writer.write_chunk(buf))
            .map_err(map_smb_err)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MediaWriter for SmbWriter {
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut this = *self;
        let Some(writer) = this.writer.take() else {
            return Err(io::Error::other("writer is closed"));
        };
        this.runtime
            .block_on(writer.finish())
            .map(|_| ())
            .map_err(map_smb_err)
    }
}

impl Drop for SmbWriter {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = self.runtime.block_on(writer.abort());
        }
    }
}

pub struct RealSmbClient {
    runtime: Arc<tokio::runtime::Runtime>,
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
            runtime: Arc::new(runtime),
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

    fn open_reader(&self, target: &SmbTarget) -> io::Result<Box<dyn MediaReader>> {
        let guard = self.inner.lock();
        let ShareSession { client, tree } = &*guard;
        let reader = self
            .runtime
            .block_on(client.open_file_reader(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)?;
        Ok(Box::new(SmbReader::new(
            Arc::clone(&self.runtime),
            reader,
            target.path.to_string(),
        )))
    }

    fn create_writer(&self, target: &SmbTarget) -> io::Result<Box<dyn MediaWriter>> {
        let guard = self.inner.lock();
        let ShareSession { client, tree } = &*guard;
        let writer = self
            .runtime
            .block_on(client.create_file_writer_exclusive(tree, path_or_root(&target.path)))
            .map_err(map_smb_err)?;
        Ok(Box::new(SmbWriter {
            runtime: Arc::clone(&self.runtime),
            writer: Some(writer),
            path: target.path.to_string(),
        }))
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
