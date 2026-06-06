//! `DefaultBackendFactory` 真实装配分支（backend feature 启用侧）测试。
//!
//! 与 `dispatch_and_cli.rs` 的 `#[cfg(not(feature = ...))]` rejection 系列互为镜像：
//! 本文件在 feature 启用时编译，否则 `--all-features` 严格覆盖率下
//! `factory.rs::for_location` 的真实 builder 调用位点与 `dispatch.rs` 的
//! `build_source(s)` / `for_location` `?` Err arm 无测试触发。
//! 确定性依据：`RealMtpClient::new` 是 stub 必 Err；`PavaoClient::new` /
//! `ADBServerDevice::new` 仅初始化不连网。

#[cfg(feature = "mtp-backend")]
use tempfile::tempdir;
#[cfg(any(
    feature = "smb-backend",
    feature = "mtp-backend",
    feature = "adb-backend"
))]
use tidymedia::{BackendFactory, DefaultBackendFactory};
#[cfg(feature = "mtp-backend")]
use tidymedia::{Commands, tidy};

#[cfg(feature = "adb-backend")]
use super::adb_loc;
#[cfg(feature = "smb-backend")]
use super::smb_loc;
#[cfg(feature = "mtp-backend")]
use super::{DATA_DIR, local, mtp_loc};

/// `PavaoClient::new` 只初始化 smbc 上下文不连接服务器；初始化成败取决于
/// 本机 libsmbclient 环境，两种结果都是确定性断言。
#[cfg(feature = "smb-backend")]
#[test]
fn factory_smb_for_location_builds_real_backend_or_reports_pavao_error() {
    match DefaultBackendFactory.for_location(&smb_loc("Inbox")) {
        Ok(backend) => assert_eq!(backend.scheme(), "smb"),
        Err(e) => {
            let msg = format!("{e}");
            assert!(msg.contains("pavao"), "got: {msg}");
        }
    }
}

/// `ADBServerDevice::new` 惰性连接，不触网即构造成功。
#[cfg(feature = "adb-backend")]
#[test]
fn factory_adb_for_location_builds_real_backend_without_network() {
    let backend = DefaultBackendFactory
        .for_location(&adb_loc("/sdcard/DCIM"))
        .expect("RealAdbClient::new must not touch the network");
    assert_eq!(backend.scheme(), "adb");
}

#[cfg(feature = "mtp-backend")]
#[test]
fn factory_mtp_for_location_errs_while_real_client_is_stub() {
    // Arc<dyn Backend> 不 impl Debug → unwrap_err 编译不过，用 let-else（rust-p1 §11）。
    let Err(err) = DefaultBackendFactory.for_location(&mtp_loc("DCIM")) else {
        panic!("mtp stub must fail to build a real backend");
    };
    let msg = format!("{err}");
    assert!(msg.contains("stub"), "got: {msg}");
}

// 以下四个测试借 mtp stub 必 Err 覆盖 dispatch.rs 各 `?` Err arm。

/// Copy sources 含 mtp → `build_sources(..)?` Err arm。
#[cfg(feature = "mtp-backend")]
#[test]
fn tidy_copy_mtp_source_propagates_factory_error() {
    let out = tempdir().unwrap();
    let err = tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![mtp_loc("DCIM")],
        output: local(out.path().to_str().unwrap()),
        archive_template: None,
        report: None,
    })
    .unwrap_err();
    assert!(format!("{err}").contains("stub"), "got: {err}");
}

/// Copy output 是 mtp → `build_source(factory, output)?` Err arm。
#[cfg(feature = "mtp-backend")]
#[test]
fn tidy_copy_mtp_output_propagates_factory_error() {
    let err = tidy(Commands::Copy {
        dry_run: true,
        include_non_media: false,
        sources: vec![local(DATA_DIR)],
        output: mtp_loc("Out"),
        archive_template: None,
        report: None,
    })
    .unwrap_err();
    assert!(format!("{err}").contains("stub"), "got: {err}");
}

/// Find sources 含 mtp → find 分支 `build_sources(..)?` Err arm。
#[cfg(feature = "mtp-backend")]
#[test]
fn tidy_find_mtp_source_propagates_factory_error() {
    let err = tidy(Commands::Find {
        secure: false,
        sources: vec![mtp_loc("DCIM")],
        output: None,
        report: None,
    })
    .unwrap_err();
    assert!(format!("{err}").contains("stub"), "got: {err}");
}

/// Find output 是 mtp → `output.map(..).transpose()?` Err arm。
#[cfg(feature = "mtp-backend")]
#[test]
fn tidy_find_mtp_output_propagates_factory_error() {
    let err = tidy(Commands::Find {
        secure: false,
        sources: vec![local(DATA_DIR)],
        output: Some(mtp_loc("Out")),
        report: None,
    })
    .unwrap_err();
    assert!(format!("{err}").contains("stub"), "got: {err}");
}
