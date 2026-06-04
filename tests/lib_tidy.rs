//! `tidy` / `tidy_with` / `run_cli` 端到端集成测试。
//! root：use + helpers + `FakeBackendFactory` + smb/mtp/adb Location 构造器。
//! 业务测试拆到 `lib_tidy/*.rs` 三个子文件以保持单文件 < 512 行（P0 §6）。

use std::collections::HashMap;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tidymedia::{Backend, BackendFactory, Error, LocalBackend, Location, Result};

const DATA_DIR: &str = "tests/data";

fn local(p: &str) -> Location {
    Location::Local(Utf8PathBuf::from(p))
}

/// 集成测试用的 BackendFactory：local scheme 给真实 LocalBackend，其他 scheme
/// 从注入 map 取 Arc<dyn Backend>（通常是 FakeBackend）；未注入 scheme 返 Unsupported。
struct FakeBackendFactory {
    by_scheme: HashMap<&'static str, Arc<dyn Backend>>,
}

impl FakeBackendFactory {
    fn new() -> Self {
        Self {
            by_scheme: HashMap::new(),
        }
    }

    fn insert(&mut self, scheme: &'static str, backend: Arc<dyn Backend>) {
        self.by_scheme.insert(scheme, backend);
    }
}

impl BackendFactory for FakeBackendFactory {
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>> {
        if let Some(b) = self.by_scheme.get(loc.scheme()) {
            return Ok(Arc::clone(b));
        }
        if matches!(loc, Location::Local(_)) {
            return Ok(LocalBackend::arc());
        }
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("no fake backend for scheme {}", loc.scheme()),
        )))
    }
}

fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "photos".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn mtp_loc(path: &str) -> Location {
    Location::Mtp {
        device: "Pixel".into(),
        storage: "Internal".into(),
        path: Utf8PathBuf::from(path),
    }
}

fn adb_loc(path: &str) -> Location {
    Location::Adb {
        serial: Some("EMULATOR5554".into()),
        path: Utf8PathBuf::from(path),
    }
}

#[path = "lib_tidy/dispatch_and_cli.rs"]
mod dispatch_and_cli;

#[path = "lib_tidy/backends.rs"]
mod backends;

#[path = "lib_tidy/archive.rs"]
mod archive;
