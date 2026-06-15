// 配置加载：从文件系统 / 环境变量读取并解析为 [`Config`]。
// Config 结构体定义在 usecases::config；本模块只负责 IO + 解析。
use std::env;
use std::fs;
use std::sync::OnceLock;

use tracing::debug;
use tracing::warn;

use crate::usecases::config::{Config, CopyConfig, LogConfig, validate_archive_template};

static CONFIG: OnceLock<Config> = OnceLock::new();

/// 全局只读配置；首次调用时加载。
pub fn config() -> &'static Config {
    CONFIG.get_or_init(load)
}

fn load() -> Config {
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

/// 非法字段值回退默认并告警，与"parse 失败回退 `Config::default`"同一哲学：
/// 配置错误不让 CLI panic 或静默全量失败，但必须可观测。
/// - `unique_name_max_attempts == 0` 会让 `generate_unique_name` 的 `0..0` 循环
///   永不执行恒返 `None`，所有 copy/move 静默失败
/// - 非法 `archive_template`（嵌套/错配/未知占位符）会渲染出字面 `{xxx}` 目录
fn sanitize(mut cfg: Config) -> Config {
    if cfg.copy.unique_name_max_attempts == 0 {
        let fallback = CopyConfig::default().unique_name_max_attempts;
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "copy.unique_name_max_attempts",
            fallback,
            "unique_name_max_attempts must be >= 1; falling back to default"
        );
        cfg.copy.unique_name_max_attempts = fallback;
    }
    if let Err(e) = validate_archive_template(&cfg.copy.archive_template) {
        let fallback = CopyConfig::default().archive_template;
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "copy.archive_template",
            error = %e,
            fallback = %fallback,
            "archive_template invalid; falling back to default"
        );
        cfg.copy.archive_template = fallback;
    }
    // 非法 level 会让 CLI 端 parse 失败静默退 info；此处统一回退 + 告警。
    // 注意：未传 --log-level 时 config 在 subscriber 安装前加载，本 warn 不可见
    //（行为仍安全回退）；显式传 flag 或 RUST_LOG 时可见。
    if cfg.log.level.parse::<tracing::Level>().is_err() {
        let fallback = LogConfig::default().level;
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "log.level",
            value = %cfg.log.level,
            fallback = %fallback,
            "log.level must be one of trace/debug/info/warn/error; falling back to default"
        );
        cfg.log.level = fallback;
    }
    cfg
}

/// 把 `${VAR:-default}` 替换为环境变量值或默认值。
//
// `$` `{` `}` 都是 ASCII，UTF-8 多字节字符的字节绝不会撞上 ASCII 范围；
// 因此按字节扫描 placeholder 边界，剩余段以 `&input[..]` 切片整段 push，
// 保留原 UTF-8 编码不被逐字节降级为 Latin-1。
fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'$'
            && bytes[i + 1] == b'{'
            && let Some(end) = find_close_brace(bytes, i + 2)
        {
            out.push_str(&input[last..i]);
            out.push_str(&resolve_var(&input[i + 2..end]));
            i = end + 1;
            last = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&input[last..]);
    out
}

// 按括号配对计数找闭合 `}`：默认值可含嵌套占位符
// （如 `${TMPL:-{year}/{month}}`），取第一个 `}` 会截断默认值产生非法 YAML。
fn find_close_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (off, &b) in bytes[start..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + off);
                }
            }
            _ => {}
        }
    }
    None
}

fn resolve_var(body: &str) -> String {
    if let Some((name, default)) = body.split_once(":-") {
        env::var(name).unwrap_or_else(|_| default.to_string())
    } else {
        env::var(body).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "config_test_common.rs"]
mod test_common;

#[cfg(test)]
#[path = "config_expand_tests.rs"]
mod expand_tests;

#[cfg(test)]
#[path = "config_load_tests.rs"]
mod load_tests;
