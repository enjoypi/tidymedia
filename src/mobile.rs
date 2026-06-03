//! Android / 移动端 uniffi 绑定层。
//!
//! 仅在 `--features android-app` 启用时编译；通过 `uniffi::setup_scaffolding!()`
//! 在 lib.rs 顶层一次生成 FFI scaffolding，本模块用 `#[uniffi::export]` 暴露
//! 简化的 dry-run / run 入口给 Kotlin 调用。
//!
//! 设计哲学（与 [CLI 入口] 对齐）：
//! - Kotlin 端把 SAF 选到的目录翻成本地 file path（Android 11+ 可用 `/storage/emulated/0/...`
//!   或 `MediaStore` 投影），传 `String` 给 Rust；Rust 端走与 CLI 完全一致的
//!   [`crate::tidy_with`]：[`DefaultBackendFactory`] + [`Commands::Copy`] `dry_run=true/false`
//! - 没有专门的 mobile Use Case；保留 Single Responsibility（mobile 只做 FFI 适配）
//! - 返回 [`TidyStats`]：扫到的文件总数 + 已成功复制的字节数，让 UI 显示一个数字即可
//!
//! [CLI 入口]: crate::run_cli

use camino::Utf8PathBuf;

use crate::{Commands, DefaultBackendFactory, Error, Location, tidy_with};

/// 一次 tidy 调用的简单计数，UI 仅需要这些字段。
#[derive(uniffi::Record, Clone, Debug)]
pub struct TidyStats {
    /// 扫到的源端文件总数（含被识别为非媒体而跳过的）
    pub total_scanned: u32,
    /// `实际写入目标端的文件数（dry_run=true` 时永远为 0）
    pub copied: u32,
    /// `dry_run` / run 模式回执给 UI；正常完成 = "ok"，被 Err 截断 = error 文案
    pub status: String,
}

/// uniffi 暴露给 Kotlin 的统一错误。
/// 将 [`crate::Error`] 收敛成单一变体携带文案；UI 仅展示文案不做结构化匹配。
/// 字段名故意用 `text` 而非 `message`：uniffi 0.31 在 Kotlin 端把 `message`
/// 字段渲染成 `val message: String`，会与 `kotlin.Exception.message` 撞名导致编译失败。
#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum TidyError {
    #[error("{text}")]
    Generic { text: String },
}

impl From<Error> for TidyError {
    fn from(e: Error) -> Self {
        Self::Generic {
            text: format!("{e}"),
        }
    }
}

/// Dry-run：扫源 / 找重复，但不写目标，不删源。
/// Kotlin 调用约定：传 `src` 是设备上绝对路径（如 `/storage/emulated/0/DCIM`），
/// `output` 同样是本地路径但 dry-run 下不实际写。
///
/// # Errors
///
/// 当扫描源、解析路径或底层 `tidy_with` 执行失败时返回 `TidyError`。
#[uniffi::export]
pub fn tidy_dry_run(src: String, output: String) -> Result<TidyStats, TidyError> {
    run_internal(src, output, /* dry_run = */ true)
}

/// 真实跑：扫源 → 复制非重复媒体到 output，不删源（move 模式不在 P1 范围）。
///
/// # Errors
///
/// 当扫描源、解析路径或底层 `tidy_with` 执行失败时返回 `TidyError`。
#[uniffi::export]
pub fn tidy_run(src: String, output: String) -> Result<TidyStats, TidyError> {
    run_internal(src, output, /* dry_run = */ false)
}

/// 版本号给 UI 显示用，便于排查"App 里 Rust core 哪个版本"。
#[uniffi::export]
#[must_use]
pub fn tidymedia_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn run_internal(src: String, output: String, dry_run: bool) -> Result<TidyStats, TidyError> {
    let src_loc = Location::Local(Utf8PathBuf::from(src));
    let out_loc = Location::Local(Utf8PathBuf::from(output));
    let cmd = Commands::Copy {
        dry_run,
        include_non_media: false,
        sources: vec![src_loc],
        output: out_loc,
    };
    tidy_with(&DefaultBackendFactory, cmd)?;
    // tidy_with 内部已经把 stats 打到 stdout / debug log；本层先只回执 status，
    // total_scanned / copied 暂留 0——结构化导出需要从 use case 内回传计数。
    Ok(TidyStats {
        total_scanned: 0,
        copied: 0,
        status: if dry_run {
            "dry-run ok".into()
        } else {
            "ok".into()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_cargo_pkg_version() {
        assert_eq!(tidymedia_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn dry_run_on_empty_dir_returns_ok_status() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let stats = tidy_dry_run(
            src.path().to_str().unwrap().into(),
            out.path().to_str().unwrap().into(),
        )
        .unwrap();
        assert_eq!(stats.status, "dry-run ok");
        assert_eq!(stats.total_scanned, 0);
        assert_eq!(stats.copied, 0);
    }

    #[test]
    fn run_on_empty_dir_returns_ok_status() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let stats = tidy_run(
            src.path().to_str().unwrap().into(),
            out.path().to_str().unwrap().into(),
        )
        .unwrap();
        assert_eq!(stats.status, "ok");
    }

    #[test]
    fn tidy_error_carries_underlying_message() {
        let err: TidyError = Error::Io(std::io::Error::other("boom")).into();
        let TidyError::Generic { text } = err;
        assert!(text.contains("boom"), "got: {text}");
    }

    #[test]
    fn tidy_stats_record_fields_clone_and_debug() {
        let s = TidyStats {
            total_scanned: 7,
            copied: 3,
            status: "ok".into(),
        };
        let s2 = s.clone();
        assert_eq!(s2.total_scanned, 7);
        assert_eq!(s2.copied, 3);
        assert!(format!("{s:?}").contains("TidyStats"));
    }
}
