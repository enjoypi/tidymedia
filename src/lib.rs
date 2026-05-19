#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod adapters;
mod entities;
mod frameworks;
mod usecases;

// ── Interface Adapters re-exports ──
pub use adapters::backend::factory::{BackendFactory, DefaultBackendFactory};
pub use adapters::cli::{Cli, Commands, run_cli};
pub use adapters::dispatch::{tidy, tidy_with};

// ── Entity re-exports ──
pub use entities::backend::adb::{AdbBackend, AdbClient, AdbTarget};
pub use entities::backend::local::LocalBackend;
pub use entities::backend::mtp::{MtpBackend, MtpClient, MtpMatch, MtpTarget};
pub use entities::backend::smb::{SmbBackend, SmbClient, SmbTarget};
pub use entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
pub use entities::common::Error;
pub use entities::common::Result;
pub use entities::media_time;
pub use entities::uri::{Location, ParseError as LocationParseError};

#[doc(hidden)]
pub use entities::backend::fake::{FakeBackend, Op as FakeOp};

#[cfg(feature = "android-app")]
uniffi::setup_scaffolding!();

#[cfg(feature = "android-app")]
pub mod mobile;
