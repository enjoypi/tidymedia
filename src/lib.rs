// 严格覆盖率模式：跑 `RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov nextest`
// 时启用，让带有 `#[cfg_attr(coverage_nightly, coverage(off))]` 的函数被 LLVM 跳过统计。
// 不影响默认 stable 构建。
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::ffi::OsString;
use std::sync::Arc;

use clap::Parser;
use clap::Subcommand;
use tracing::debug;
use tracing_subscriber::fmt;
use tracing_subscriber::EnvFilter;

pub use entities::backend::local::LocalBackend;
pub use entities::backend::mtp::{MtpBackend, MtpClient, MtpMatch, MtpTarget};
pub use entities::backend::smb::{SmbBackend, SmbClient, SmbTarget};
pub use entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};

// 测试 helper：集成测试通过 FakeBackend / FakeOp 组装混合 scheme 调度。
#[doc(hidden)]
pub use entities::backend::fake::{FakeBackend, Op as FakeOp};
pub use entities::common::Error;
pub use entities::common::Result;
pub use entities::media_time;
pub use entities::uri::{Location, ParseError as LocationParseError};

mod entities;
mod usecases;

const FEATURE_CLI: &str = "cli";

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long, default_value = "info")]
    pub log_level: tracing::Level,

    #[arg(long, default_value = "false")]
    pub log_line_number: bool,

    #[arg(long, default_value = "false")]
    pub log_target: bool,

    #[arg(long, default_value = "false")]
    pub log_thread_ids: bool,

    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Copy non-duplicate media files (images / videos recognized via magic-bytes MIME) from sources to the output directory. Pass --include-non-media to also copy everything else. Duplicate detection uses SHA-512. No source files are modified. Sources / output accept URI form: `smb://[user@]host[:port]/share/path`, `mtp://device/storage/path` or plain local path.
    Copy {
        /// Dry run, do not copy files
        #[arg(short, long)]
        dry_run: bool,

        /// Also copy files that magic-bytes MIME does not classify as image/video (e.g. documents, archives, unknown formats)
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,
    },

    /// Find duplicate files under the sources and print a shell script (batch syntax on Windows) that deletes the duplicates. Default uses a fast non-cryptographic hash (xxh3-64); pass --secure to use SHA-512 instead. If --output is given, deletions for files under that directory are commented out.
    Find {
        /// Use the cryptographic hash (SHA-512) instead of the default fast non-cryptographic hash (xxh3-64). Slower but eliminates the (already astronomically small) collision risk.
        #[arg(short, long)]
        secure: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory; deletions for files under it are commented out
        #[arg(short, long)]
        output: Option<Location>,
    },

    /// Move non-duplicate media files from sources into the output directory. Sources that duplicate something already in output are physically deleted; duplicate detection uses SHA-512. Pass --include-non-media to also move everything else.
    Move {
        /// Dry run, do not move or delete files
        #[arg(short, long)]
        dry_run: bool,

        /// Also move files that magic-bytes MIME does not classify as image/video
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,
    },
}

/// Backend 装配抽象：按 [`Location`] 构造对应的 [`Backend`] 句柄。
///
/// 生产路径走 [`DefaultBackendFactory`]：Local 直接给 [`LocalBackend`]，SMB / MTP
/// 在未启用对应 feature 时报 `Unsupported`。测试用 fake 实现注入 [`FakeBackend`]
/// 覆盖跨 scheme 调度（见 `tests/lib_tidy.rs`）。
pub trait BackendFactory: Send + Sync {
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>>;
}

/// 生产 [`BackendFactory`]：根据 Location.scheme 选 backend；当前仅 Local 真实可用，
/// SMB / MTP 等真实适配器分别由 `smb-backend` / `mtp-backend` cargo feature 启用
/// （Task 4 / Task 5 接入），未启用时返 `Unsupported`。
#[derive(Debug, Default)]
pub struct DefaultBackendFactory;

impl BackendFactory for DefaultBackendFactory {
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>> {
        match loc {
            Location::Local(_) => Ok(LocalBackend::arc()),
            Location::Smb { .. } => build_smb_backend(loc),
            Location::Mtp { .. } => build_mtp_backend(loc),
        }
    }
}

#[cfg(feature = "smb-backend")]
#[cfg_attr(coverage_nightly, coverage(off))]
fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    use entities::backend::smb::real::RealSmbClient;
    let target = entities::backend::smb::SmbTarget {
        user: match loc {
            Location::Smb { user, .. } => user.clone(),
            _ => None,
        },
        host: match loc {
            Location::Smb { host, .. } => host.clone(),
            _ => String::new(),
        },
        port: match loc {
            Location::Smb { port, .. } => *port,
            _ => None,
        },
        share: match loc {
            Location::Smb { share, .. } => share.clone(),
            _ => String::new(),
        },
        path: Default::default(),
        password: std::env::var("SMB_PASSWORD").ok(),
        krb5_ccname: std::env::var("KRB5CCNAME").ok(),
    };
    let cfg = &usecases::config::config().backend.smb;
    let client = RealSmbClient::new(&target, &cfg.default_user, &cfg.workgroup)
        .map_err(Error::Io)?;
    Ok(SmbBackend::arc_with_client(Arc::new(client)))
}

#[cfg(not(feature = "smb-backend"))]
fn build_smb_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "{} backend not enabled in this build; rebuild with --features smb-backend",
            loc.scheme()
        ),
    )))
}

#[cfg(feature = "mtp-backend")]
#[cfg_attr(coverage_nightly, coverage(off))]
fn build_mtp_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    // RealMtpClient 当前是 stub：feature 启用编译通过，运行期仍返 Unsupported，
    // 错误消息指向未来 PR 选定具体 crate（libmtp-rs / gphoto2 / 自接 rusb）。
    use entities::backend::mtp::real::RealMtpClient;
    let _ = loc;
    let _ = RealMtpClient::new()?;
    unreachable!("RealMtpClient::new always returns Err in the stub phase");
}

#[cfg(not(feature = "mtp-backend"))]
fn build_mtp_backend(loc: &Location) -> Result<Arc<dyn Backend>> {
    Err(Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "{} backend not enabled in this build; rebuild with --features mtp-backend",
            loc.scheme()
        ),
    )))
}

/// 用默认 backend factory 跑命令；旧入口，等价于 `tidy_with(&DefaultBackendFactory, ...)`。
pub fn tidy(command: Commands) -> Result<()> {
    tidy_with(&DefaultBackendFactory, command)
}

/// 注入版入口：调用方提供 [`BackendFactory`]，常用于集成测试用 fake 装配混合 scheme。
pub fn tidy_with(factory: &dyn BackendFactory, command: Commands) -> Result<()> {
    match command {
        Commands::Copy {
            dry_run,
            include_non_media,
            sources,
            output,
        } => {
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = build_source(factory, output)?;
            usecases::copy(src_pairs, out_pair, dry_run, false, include_non_media)
        }
        Commands::Find {
            secure,
            sources,
            output,
        } => {
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = output
                .map(|loc| build_source(factory, loc))
                .transpose()?;
            usecases::find_duplicates(secure, src_pairs, out_pair)
        }
        Commands::Move {
            dry_run,
            include_non_media,
            sources,
            output,
        } => {
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = build_source(factory, output)?;
            usecases::copy(src_pairs, out_pair, dry_run, true, include_non_media)
        }
    }
}

fn build_source(factory: &dyn BackendFactory, loc: Location) -> Result<usecases::Source> {
    let backend = factory.for_location(&loc)?;
    Ok((loc, backend))
}

fn build_sources(
    factory: &dyn BackendFactory,
    locs: Vec<Location>,
) -> Result<Vec<usecases::Source>> {
    locs.into_iter().map(|loc| build_source(factory, loc)).collect()
}

pub fn run_cli<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = e.print();
            return Ok(());
        }
        Err(e) => {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                e.to_string(),
            )));
        }
    };
    install_logging(&cli);
    debug!(
        feature = FEATURE_CLI,
        operation = "parse_args",
        result = "ok",
        log_level = %cli.log_level,
        command = ?cli.command,
        "cli parsed"
    );
    tidy(cli.command)
}

fn install_logging(cli: &Cli) {
    let format = fmt::format()
        .with_ansi(false)
        .with_level(false)
        .with_line_number(cli.log_line_number)
        .with_target(cli.log_target)
        .with_thread_ids(cli.log_thread_ids)
        .compact();

    // 默认让 tidymedia 走 --log-level（默认 info），同时把 nom_exif 内部噪声
    // （parse_gps "find" info、"GPSInfo not found" warn 等）压到 error。
    // 用户可通过 RUST_LOG 覆盖（如 RUST_LOG=nom_exif=debug）。
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("{}={},nom_exif=error", env!("CARGO_PKG_NAME"), cli.log_level))
    });

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .event_format(format)
        .try_init();
}
