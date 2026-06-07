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
mod tests {
    use super::*;

    #[test]
    fn expand_env_substitutes_default_when_var_missing() {
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_MISSING_VAR_X") };
        let s = expand_env("a: ${TIDYMEDIA_TEST_MISSING_VAR_X:-7}");
        assert_eq!(s, "a: 7");
    }

    /// placeholder 位于串首（i=0）：既有用例 `${` 全在 i>=2 处，杀不掉
    /// `find_close_brace(bytes, i + 2)` 被变异成 `i - 2`（i>=2 时搜索结果恰好相同；
    /// i=0 时 usize 下溢 → 越界 panic）。
    #[test]
    fn expand_env_placeholder_at_string_start_expands() {
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_START_VAR") };
        assert_eq!(expand_env("${TIDYMEDIA_TEST_START_VAR:-dft}"), "dft");
    }

    #[test]
    fn expand_env_uses_env_value_when_set() {
        unsafe { std::env::set_var("TIDYMEDIA_TEST_SET_VAR_Y", "42") };
        let s = expand_env("a: ${TIDYMEDIA_TEST_SET_VAR_Y:-0}");
        assert_eq!(s, "a: 42");
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_SET_VAR_Y") };
    }

    #[test]
    fn expand_env_resolves_bare_name_without_default() {
        unsafe { std::env::set_var("TIDYMEDIA_TEST_BARE_Z", "hi") };
        let s = expand_env("k: ${TIDYMEDIA_TEST_BARE_Z}");
        assert_eq!(s, "k: hi");
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_BARE_Z") };
    }

    #[test]
    fn expand_env_keeps_text_without_placeholder() {
        assert_eq!(expand_env("plain: text"), "plain: text");
    }

    // 默认值含嵌套 `{}`（如 archive_template 的占位符）：必须按括号配对找
    // 闭合 `}`，否则截断成 `{year` 产生非法 YAML（真实 config.yaml:10 的回归）。
    #[test]
    fn expand_env_default_with_nested_braces_expands_fully() {
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_NESTED_TMPL") };
        let s = expand_env("t: \"${TIDYMEDIA_TEST_NESTED_TMPL:-{year}/{month}/{valuable_name}}\"");
        assert_eq!(s, "t: \"{year}/{month}/{valuable_name}\"");
    }

    // 嵌套 `{` 打开后未闭合：depth 永不归零，整段保留原文不替换。
    #[test]
    fn expand_env_leaves_unbalanced_nested_braces() {
        assert_eq!(expand_env("a: ${VAR:-{x}"), "a: ${VAR:-{x}");
    }

    #[test]
    fn expand_env_leaves_unterminated_brace() {
        assert_eq!(expand_env("a: ${UNCLOSED"), "a: ${UNCLOSED");
    }

    #[test]
    fn expand_env_handles_trailing_dollar() {
        assert_eq!(expand_env("a$"), "a$");
    }

    // 触发 `bytes[i + 1] == b'{'` 的 False 分支：`$` 后跟非 `{` 字符且不在末尾。
    #[test]
    fn expand_env_dollar_not_followed_by_brace_passes_through() {
        assert_eq!(expand_env("$abc"), "$abc");
        assert_eq!(expand_env("price: $9.99"), "price: $9.99");
    }

    // 防御 UTF-8 字符被逐字节降级为 Latin-1（旧实现 `bytes[i] as char` 的 bug）。
    #[test]
    fn expand_env_preserves_multibyte_chars() {
        unsafe { std::env::set_var("TIDYMEDIA_TEST_UTF8_K", "值") };
        let s = expand_env("# 目标文件 \nk: ${TIDYMEDIA_TEST_UTF8_K:-默认}");
        assert_eq!(s, "# 目标文件 \nk: 值");
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_UTF8_K") };
    }

    #[test]
    fn expand_env_preserves_multibyte_default() {
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_UTF8_MISS") };
        let s = expand_env("k: ${TIDYMEDIA_TEST_UTF8_MISS:-默认值}");
        assert_eq!(s, "k: 默认值");
    }

    // 真实 config.yaml 中常出现一行多占位符（如 `host: ${HOST:-...} port: ${PORT:-...}`），
    // 既验证两次替换都生效，也防御「第二个 `${` 起点定位」回归。
    #[test]
    fn expand_env_handles_multiple_placeholders_on_same_line() {
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_MULTI_HOST") };
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_MULTI_PORT") };
        let both_missing = expand_env(
            "host: ${TIDYMEDIA_TEST_MULTI_HOST:-127.0.0.1} port: ${TIDYMEDIA_TEST_MULTI_PORT:-5037}",
        );
        assert_eq!(both_missing, "host: 127.0.0.1 port: 5037");

        unsafe { std::env::set_var("TIDYMEDIA_TEST_MULTI_HOST", "example.com") };
        let host_set = expand_env(
            "host: ${TIDYMEDIA_TEST_MULTI_HOST:-127.0.0.1} port: ${TIDYMEDIA_TEST_MULTI_PORT:-5037}",
        );
        assert_eq!(host_set, "host: example.com port: 5037");
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_MULTI_HOST") };
    }

    #[test]
    fn resolve_var_missing_no_default_returns_empty() {
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_NO_DEFAULT_W") };
        assert_eq!(resolve_var("TIDYMEDIA_TEST_NO_DEFAULT_W"), "");
    }

    // yaml 故意保留已删除的 timeout_secs / mtp 节：serde 默认忽略未知字段，
    // 旧 config.yaml 必须保持向后兼容不报错。
    #[test]
    fn backend_config_yaml_overrides_defaults_and_ignores_removed_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.yaml");
        std::fs::write(
            &path,
            "backend:\n  smb:\n    default_user: alice\n    workgroup: HOME\n    timeout_secs: 60\n  mtp:\n    device_match: exact\n    storage_match: exact\n  adb:\n    server_host: 10.0.0.5\n    server_port: 15037\n    timeout_secs: 90\n",
        )
        .unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.backend.smb.default_user, "alice");
        assert_eq!(cfg.backend.smb.workgroup, "HOME");
        assert_eq!(cfg.backend.adb.server_host, "10.0.0.5");
        assert_eq!(cfg.backend.adb.server_port, 15037);
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    #[test]
    fn load_falls_back_when_file_missing() {
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", "/no/such/file/xyz.yaml") };
        let cfg = load();
        assert_eq!(cfg.copy.timezone_offset_hours, 8);
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    #[test]
    fn load_falls_back_when_yaml_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.yaml");
        std::fs::write(&path, "::: not yaml :::").unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.copy.unique_name_max_attempts, 10);
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    #[test]
    fn load_reads_explicit_values_via_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.yaml");
        std::fs::write(
            &path,
            "copy:\n  timezone_offset_hours: 0\n  unique_name_max_attempts: 5\nexif:\n  valid_date_time_secs: 100\n",
        )
        .unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.copy.timezone_offset_hours, 0);
        assert_eq!(cfg.copy.unique_name_max_attempts, 5);
        assert_eq!(cfg.exif.valid_date_time_secs, 100);
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    #[test]
    fn config_global_accessor_returns_static() {
        let a = config();
        let b = config();
        assert!(std::ptr::eq(a, b));
    }

    // max_attempts=0 会让 generate_unique_name 恒返 None（copy 静默全量失败），
    // load 必须回退默认值。
    #[test]
    fn load_sanitizes_zero_unique_name_max_attempts_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zero.yaml");
        std::fs::write(&path, "copy:\n  unique_name_max_attempts: 0\n").unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.copy.unique_name_max_attempts, 10);
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    // yaml 内非法模板（结构错配）回退默认模板，不让渲染产生字面 '{' 目录。
    #[test]
    fn load_sanitizes_invalid_archive_template_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("badtmpl.yaml");
        std::fs::write(&path, "copy:\n  archive_template: \"{year/{month}}\"\n").unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.copy.archive_template, "{year}/{month}/{valuable_name}");
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    // 非法 log.level 回退 "info"，不让 CLI 端 parse 静默吞掉配置错误。
    #[test]
    fn load_sanitizes_invalid_log_level_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("badlevel.yaml");
        std::fs::write(&path, "log:\n  level: chatty\n").unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.log.level, "info");
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    // 合法 log.level 不被 sanitize 改写（防无条件重置变异）。
    #[test]
    fn load_keeps_valid_log_level_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oklevel.yaml");
        std::fs::write(&path, "log:\n  level: debug\n").unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.log.level, "debug");
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    // 端到端回归：真实 config.yaml 写法（带引号 + 嵌套占位符默认值）必须
    // 解析成功，不触发 parse_error 回退。
    #[test]
    fn load_parses_quoted_template_placeholder_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tmpl.yaml");
        std::fs::write(
            &path,
            "copy:\n  archive_template: \"${TIDYMEDIA_TEST_LOAD_TMPL:-{year}/{day}}\"\n  unique_name_max_attempts: 4\n",
        )
        .unwrap();
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_LOAD_TMPL") };
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.copy.archive_template, "{year}/{day}");
        // 同文件其余字段未因 parse_error 丢失
        assert_eq!(cfg.copy.unique_name_max_attempts, 4);
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }

    // 合法配置不被 sanitize 改写（防 sanitize 被变异成无条件重置）。
    #[test]
    fn load_keeps_valid_copy_fields_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid.yaml");
        std::fs::write(
            &path,
            "copy:\n  unique_name_max_attempts: 3\n  archive_template: \"{year}/{day}\"\n",
        )
        .unwrap();
        unsafe { std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap()) };
        let cfg = load();
        assert_eq!(cfg.copy.unique_name_max_attempts, 3);
        assert_eq!(cfg.copy.archive_template, "{year}/{day}");
        unsafe { std::env::remove_var("TIDYMEDIA_CONFIG") };
    }
}
