// 配置加载：从文件系统 / 环境变量读取并解析为 [`Config`]。
// Config 结构体 + 全局 OnceLock + 访问器在 usecases::config（应用关注点）；
// 本模块只负责 IO + 解析（外部数据格式适配器），通过 [`install_global_loader`]
// 把 [`load`] 装到 usecases 层供 lazy init 使用。
//
// 拆分为三文件（原 518 行 → ≤300）：
// - `config_sanitize`：非法值回退默认（sanitize 族）
// - `config_expand`：`${VAR:-default}` 展开
// - 本文件：load 入口 + loader 装配
#[path = "config_expand.rs"]
mod config_expand;
#[path = "config_sanitize.rs"]
mod config_sanitize;

use std::env;
use std::fs;

use tracing::debug;
use tracing::warn;

use crate::usecases::config::Config;

pub(super) use config_expand::expand_env;
#[cfg(test)]
pub(super) use config_expand::resolve_var;
pub(super) use config_sanitize::sanitize;

/// 把 yaml/env loader 注入 `usecases::config` 全局；CLI / FFI 启动早期调用。
/// 多次调用静默忽略后续（OnceLock 语义）。
pub fn install_global_loader() {
    crate::usecases::config::install_loader(load);
}

/// 读 yaml + 解析 + sanitize 出一份 [`Config`]。文件缺失或解析失败回退 [`Config::default`]。
/// `pub(crate)` 让 `lib_tidy` 集成测试 binary 通过 `install_global_loader` 间接走此路径；
/// 直接调 [`load`] 的 lib unit 测试位于 `config_load_tests.rs`。
pub(crate) fn load() -> Config {
    let path = env::var("TIDYMEDIA_CONFIG").unwrap_or_else(|_| "config.yaml".to_string());

    let Ok(raw) = fs::read_to_string(&path) else {
        debug!(
            feature = "config",
            operation = "load",
            result = "fallback_default",
            path = %path,
            "config file missing, using defaults"
        );
        return Config::default();
    };

    let expanded = expand_env(&raw);
    match serde_yaml::from_str::<Config>(&expanded) {
        Ok(cfg) => {
            debug!(
                feature = "config",
                operation = "load",
                result = "ok",
                path = %path,
                "config loaded"
            );
            sanitize(cfg)
        }
        Err(e) => {
            warn!(
                feature = "config",
                operation = "load",
                result = "parse_error",
                path = %path,
                error = %e,
                "config parse failed, falling back to defaults"
            );
            Config::default()
        }
    }
}

#[cfg(test)]
#[path = "config_test_helpers.rs"]
mod test_common;

#[cfg(test)]
#[path = "config_expand_tests.rs"]
mod expand_tests;

#[cfg(test)]
#[path = "config_load_tests.rs"]
mod load_tests;
