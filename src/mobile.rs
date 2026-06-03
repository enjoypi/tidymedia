//! Android / 移动端 uniffi 绑定层。
//!
//! 仅在 `--features android-app` 启用时编译；通过 `uniffi::setup_scaffolding!()`
//! 在 lib.rs 顶层一次生成 FFI scaffolding，本模块用 `#[uniffi::export]` 暴露
//! 简化的 dry-run / run / find-duplicates 入口给 Kotlin 调用。
//!
//! 设计哲学（与 [CLI 入口] 对齐）：
//! - Kotlin 端把 SAF 选到的目录翻成本地 file path（Android 11+ 可用
//!   `/storage/emulated/0/...` 或 `MediaStore` 投影），传 `String` 给 Rust；
//!   Rust 端走与 CLI 完全一致的路径：`DefaultBackendFactory`
//! - 没有专门的 mobile Use Case；保留 Single Responsibility（mobile 只做 FFI 适配）
//! - `sources` 字符串接受与 CLI 相同的 URI 语法（`smb://...` / `adb://...` /
//!   本地路径），由 `Location::from_str` 解析，Kotlin 端无需感知 backend 细节
//!
//! [CLI 入口]: crate::run_cli

use std::str::FromStr;

use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::adapters::dispatch::{copy_report, find_report};
use crate::entities::uri::Location;

/// 一次 tidy copy 调用的统计。UI 用这些字段显示进度。
#[derive(uniffi::Record, Clone, Debug)]
pub struct TidyStats {
    /// 扫到的源端文件总数（含被识别为非媒体而跳过的）
    pub total_scanned: u32,
    /// 实际写入目标端的文件数（`dry_run=true` 时永远为 0）
    pub copied: u32,
    /// 被跳过的文件数（重复 / 非媒体）
    pub ignored: u32,
    /// 复制失败的文件数
    pub failed: u32,
    /// 操作状态：正常完成 = `"ok"` / dry-run = `"dry-run ok"` / 被 Err 截断 = error 文案
    pub status: String,
}

/// find-duplicates 操作的简要报告。
#[derive(uniffi::Record, Clone, Debug)]
pub struct MobileFindReport {
    /// 参与查重的文件总数（所有重复组内路径数之和）
    pub scanned: u32,
    /// 重复组数量
    pub group_count: u32,
    /// 每组的文件路径列表（展平后逗号拼接；UI 仅需计数时忽略即可）
    pub groups: Vec<String>,
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

impl From<crate::Error> for TidyError {
    fn from(e: crate::Error) -> Self {
        Self::Generic {
            text: format!("{e}"),
        }
    }
}

/// Dry-run：扫源 / 找重复，但不写目标，不删源。
/// Kotlin 调用约定：`src` 接受本地绝对路径或 URI（`smb://...` / `adb://...`），
/// `output` 同样是路径或 URI，dry-run 下不实际写。
///
/// # Errors
///
/// 当扫描源、解析路径或底层执行失败时返回 `TidyError`。
#[uniffi::export]
// uniffi 0.31 FFI 边界要求 owned String；clippy::needless_pass_by_value 只在
// android-app feature 启用时触发，跨 feature 差异须用 #[allow] 不用 #[expect]。
#[allow(clippy::needless_pass_by_value)]
pub fn tidy_dry_run(src: String, output: String) -> Result<TidyStats, TidyError> {
    run_copy_internal(&src, &output, /* dry_run = */ true)
}

/// 真实跑：扫源 → 复制非重复媒体到 output，不删源（move 模式不在 P1 范围）。
///
/// # Errors
///
/// 当扫描源、解析路径或底层执行失败时返回 `TidyError`。
#[uniffi::export]
#[allow(clippy::needless_pass_by_value)]
pub fn tidy_run(src: String, output: String) -> Result<TidyStats, TidyError> {
    run_copy_internal(&src, &output, /* dry_run = */ false)
}

/// 查找重复文件。`sources` 接受本地路径或 URI 列表，`secure` 为 true 时用 SHA-512。
///
/// # Errors
///
/// 当解析路径或底层执行失败时返回 `TidyError`。
#[uniffi::export]
#[allow(clippy::needless_pass_by_value)]
pub fn tidy_find_duplicates(
    sources: Vec<String>,
    secure: bool,
) -> Result<MobileFindReport, TidyError> {
    let locs = parse_locations(sources)?;
    let report = find_report(&DefaultBackendFactory, locs, secure)?;
    Ok(MobileFindReport {
        scanned: u32::try_from(report.scanned).unwrap_or(u32::MAX),
        group_count: u32::try_from(report.groups.len()).unwrap_or(u32::MAX),
        groups: report
            .groups
            .into_iter()
            .map(|paths| paths.join(","))
            .collect(),
    })
}

/// 版本号给 UI 显示用，便于排查"App 里 Rust core 哪个版本"。
#[uniffi::export]
#[must_use]
pub fn tidymedia_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// 解析 URI 字符串列表为 Location 列表；任意一个失败即返回 TidyError。
fn parse_locations(sources: Vec<String>) -> Result<Vec<Location>, TidyError> {
    sources
        .into_iter()
        .map(|s| {
            Location::from_str(&s).map_err(|e| TidyError::Generic {
                text: format!("invalid source URI {s:?}: {e}"),
            })
        })
        .collect()
}

fn run_copy_internal(src: &str, output: &str, dry_run: bool) -> Result<TidyStats, TidyError> {
    let src_loc = Location::from_str(src).map_err(|e| TidyError::Generic {
        text: format!("invalid source URI {src:?}: {e}"),
    })?;
    let out_loc = Location::from_str(output).map_err(|e| TidyError::Generic {
        text: format!("invalid output URI {output:?}: {e}"),
    })?;
    let report = copy_report(&DefaultBackendFactory, vec![src_loc], out_loc, dry_run)?;
    Ok(TidyStats {
        total_scanned: u32::try_from(report.scanned).unwrap_or(u32::MAX),
        copied: u32::try_from(report.copied).unwrap_or(u32::MAX),
        ignored: u32::try_from(report.ignored).unwrap_or(u32::MAX),
        failed: u32::try_from(report.failed).unwrap_or(u32::MAX),
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
    fn dry_run_returns_real_scanned_count() {
        let src = tempfile::tempdir().unwrap();
        let png_src = std::path::Path::new(crate::entities::test_common::DATA_DIR)
            .join("sample-with-exif.jpg");
        std::fs::copy(&png_src, src.path().join("img.jpg")).unwrap();
        let out = tempfile::tempdir().unwrap();
        let stats = tidy_dry_run(
            src.path().to_str().unwrap().into(),
            out.path().to_str().unwrap().into(),
        )
        .unwrap();
        // dry-run で実ファイルがなければ total_scanned は 1（スキャン済み）。
        // dry-run でも copy カウンタは「コピー予定数」を返す（実ファイル作成なし）。
        assert_eq!(stats.total_scanned, 1);
        assert_eq!(stats.status, "dry-run ok");
        // 出力ディレクトリにファイルが実際に書き込まれていないことで dry-run を確認
        let written: Vec<_> = std::fs::read_dir(out.path()).unwrap().collect();
        assert!(written.is_empty(), "dry-run must not write to output dir");
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
        assert_eq!(stats.total_scanned, 0);
    }

    #[test]
    fn tidy_error_carries_underlying_message() {
        let err: TidyError = crate::Error::Io(std::io::Error::other("boom")).into();
        let TidyError::Generic { text } = err;
        assert!(text.contains("boom"), "got: {text}");
    }

    #[test]
    fn tidy_stats_record_fields_clone_and_debug() {
        let s = TidyStats {
            total_scanned: 7,
            copied: 3,
            ignored: 2,
            failed: 1,
            status: "ok".into(),
        };
        let s2 = s.clone();
        assert_eq!(s2.total_scanned, 7);
        assert_eq!(s2.copied, 3);
        assert!(format!("{s:?}").contains("TidyStats"));
    }

    #[test]
    fn find_duplicates_on_empty_dir_returns_empty_report() {
        let src = tempfile::tempdir().unwrap();
        let report =
            tidy_find_duplicates(vec![src.path().to_str().unwrap().into()], false).unwrap();
        assert_eq!(report.scanned, 0);
        assert_eq!(report.group_count, 0);
        assert!(report.groups.is_empty());
    }

    #[test]
    fn find_duplicates_report_clone_and_debug() {
        let r = MobileFindReport {
            scanned: 4,
            group_count: 2,
            groups: vec!["a,b".into(), "c,d".into()],
        };
        let r2 = r.clone();
        assert_eq!(r2.group_count, 2);
        assert!(format!("{r:?}").contains("MobileFindReport"));
    }

    #[test]
    fn find_duplicates_invalid_source_returns_err() {
        let err = tidy_find_duplicates(vec!["/nonexistent_xyz_dir".into()], false);
        // /nonexistent_xyz_dir は存在しないが LocalBackend は visit で
        // エラーをスキップするため Ok(empty) が返る — エラー経路は
        // URI パース失敗で確認する
        let _ = err; // Ok or Err どちらでもよい
        // URI パースエラーを確認
        let parse_err = tidy_find_duplicates(vec!["smb://".into()], false);
        assert!(parse_err.is_err());
    }

    #[test]
    fn dry_run_invalid_src_uri_returns_err() {
        let out = tempfile::tempdir().unwrap();
        let result = tidy_dry_run("smb://".into(), out.path().to_str().unwrap().into());
        assert!(result.is_err());
    }

    #[test]
    fn dry_run_invalid_output_uri_returns_err() {
        let src = tempfile::tempdir().unwrap();
        let result = tidy_dry_run(src.path().to_str().unwrap().into(), "smb://".into());
        assert!(result.is_err());
    }

    #[test]
    fn local_uri_scheme_accepted_in_dry_run() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let src_uri = format!("local://{}", src.path().to_str().unwrap());
        let out_uri = format!("local://{}", out.path().to_str().unwrap());
        let stats = tidy_dry_run(src_uri, out_uri).unwrap();
        assert_eq!(stats.status, "dry-run ok");
    }
}
