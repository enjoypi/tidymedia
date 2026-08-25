// 运行时配置结构体定义 + 默认值 + 全局访问器。
// 解析顺序：硬编码默认值 -> config.yaml(若存在) -> 环境变量替换 `${VAR:-default}`。
// yaml IO + sanitize 实现在 frameworks::config，按依赖倒置注入：frameworks 启动
// 时调 `frameworks::config::install_global_loader()` 把 `load` fn 装到本模块的
// [`LOADER`]；lazy init 命中时取出执行。usecases 不直接依赖 frameworks。
//
// 拆分为两文件（原 352 行 → ≤300）：
// - `config_models`：全部 Config 子结构体 + Default impl
// - 本文件：`Config` 主结构体 + OnceLock + 全局访问器 + 时区/模板工具

use std::sync::Mutex;
use std::sync::PoisonError;

use chrono::{FixedOffset, Offset, Utc};
use serde_derive::Deserialize;
use time::UtcOffset;

#[path = "config_models.rs"]
mod config_models;
#[allow(unused_imports)]
// Adb/Smb 结构体仅经 `BackendConfig` 字段被消费，仓内无 `usecases::config::<Type>` 命名引用
pub use config_models::{
    AdbBackendConfig, BackendConfig, CategoryDef, ClassifyConfig, CopyConfig, ExifConfig,
    FaceConfig, LogConfig, OcrConfig, SmbBackendConfig,
};

// `Mutex<Option<&'static Config>>` + `Box::leak`：config()` 永远返回可被安全解引用的
// `&'static Config`（leak 后永活，reset 只换当前指针，旧引用仍指向旧配置、不悬垂）；
// 比 `OnceLock` 多一个测试可用的重载入口，且无 unsafe。
static CONFIG: Mutex<Option<&'static Config>> = Mutex::new(None);
static LOADER: Mutex<Option<fn() -> Config>> = Mutex::new(None);

/// 全局只读配置；首次访问时取 [`LOADER`] 加载，未装则用 [`Config::default`]。
/// CLI / FFI 启动期 MUST 先调 `crate::install_config_loader()`，否则用户的
/// yaml 不会生效。
pub fn config() -> &'static Config {
    let mut guard = CONFIG.lock().unwrap_or_else(PoisonError::into_inner);
    guard.get_or_insert_with(|| {
        let ldr = LOADER.lock().unwrap_or_else(PoisonError::into_inner);
        let loader: Option<fn() -> Config> = *ldr;
        Box::leak(Box::new(loader.map_or_else(Config::default, |f| f())))
    })
}

/// 注入加载器（fn pointer）；frameworks 层调用，依赖倒置入口。
/// 重复调用覆盖旧 loader（生产只调用一次；测试依赖覆盖语义）。
pub fn install_loader(loader: fn() -> Config) {
    *LOADER.lock().unwrap_or_else(PoisonError::into_inner) = Some(loader);
}

/// 测试专用：丢弃当前缓存的 config 与 loader，令下一次 [`config`] 按新的
/// `TIDYMEDIA_CONFIG` env 重新加载。`write_temp_config` 这类每测试换 yaml 的
/// 场景在共享进程（cargo test）下必须先调用再 `set_var` + `install_config_loader`。
/// 旧 `&'static Config` 引用仍有效（Box::leak 永活），无悬垂；仅测试使用。
#[doc(hidden)]
pub fn reset_config_loader() {
    *CONFIG.lock().unwrap_or_else(PoisonError::into_inner) = None;
    *LOADER.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

// 时区 offset 单点：copy / move / verify 共用同一份 `timezone_offset_hours` 配置。
// 越界回退 UTC 防 panic（time::UtcOffset 合法 ±25:59:59）。
#[must_use]
pub fn offset_from_hours(hours: i8) -> UtcOffset {
    UtcOffset::from_whole_seconds(i32::from(hours) * 3600).unwrap_or(UtcOffset::UTC)
}

// chrono::FixedOffset 版：把 EXIF / 文件名内无时区的 NaiveDateTime 当相机本地时间
// 解释；与 time::UtcOffset 共用同一份 timezone_offset_hours 配置。
#[must_use]
pub fn chrono_offset_from_hours(hours: i8) -> FixedOffset {
    FixedOffset::east_opt(i32::from(hours) * 3600).unwrap_or_else(|| Utc.fix())
}

/// 默认归档模板：`{year}/{month}/{valuable_name}`。
/// `{valuable_name}` 为路径中首个含非 ASCII 的目录段；若不存在则该段为空串。
pub const DEFAULT_ARCHIVE_TEMPLATE: &str = "{year}/{month}/{valuable_name}";
pub const DEFAULT_DOC_ARCHIVE_TEMPLATE: &str = "{category}/{year}/{month}";

/// 校验归档模板：非空 + `{` `}` 结构配对 + 占位符名属已知集合。
///
/// 结构扫描替代旧的字符计数：`{year/{month}}` 计数配平但渲染时占位符无法
/// 整 token 匹配，会静默产生字面 `{year` 目录；未知占位符（如 `{foo}`）同理。
///
/// # Errors
///
/// 模板为空、花括号嵌套/错配/未闭合、或占位符名未知时返回 `Err`。
pub fn validate_archive_template(template: &str) -> Result<(), String> {
    // 保证渲染后至少有一段非空目录的占位符——剩下的 {valuable_name} 可能渲染为
    // 空串致全部文件落 output 根；要求模板含至少一个 always-non-empty 占位符。
    // const 须放 fn 顶（clippy::items_after_statements，pedantic）。
    // category 恒非空：调用方以 "uncategorized" 兜底（archive_template.rs）。
    const ALWAYS_NON_EMPTY: [&str; 6] = ["year", "month", "day", "make", "model", "category"];
    if template.is_empty() {
        return Err("archive_template must not be empty".into());
    }
    let mut start: Option<usize> = None;
    let mut has_safe_placeholder = false;
    for (i, c) in template.char_indices() {
        match c {
            '{' if start.is_some() => {
                return Err("archive_template has unbalanced braces: nested '{'".into());
            }
            '{' => start = Some(i + 1),
            '}' => {
                let Some(s) = start.take() else {
                    return Err("archive_template has unbalanced braces: unmatched '}'".into());
                };
                let name = &template[s..i];
                if !crate::usecases::archive_template::PLACEHOLDERS.contains(&name) {
                    return Err(format!(
                        "archive_template has unknown placeholder {{{name}}}"
                    ));
                }
                if ALWAYS_NON_EMPTY.contains(&name) {
                    has_safe_placeholder = true;
                }
            }
            _ => {}
        }
    }
    if start.is_some() {
        return Err("archive_template has unbalanced braces: unclosed '{'".into());
    }
    if !has_safe_placeholder {
        return Err("archive_template must contain at least one of \
             {year}/{month}/{day}/{make}/{model} to guarantee a non-empty subdirectory"
            .into());
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub copy: CopyConfig,
    pub exif: ExifConfig,
    pub backend: BackendConfig,
    pub log: LogConfig,
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
