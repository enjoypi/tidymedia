// 配置加载：从文件系统 / 环境变量读取并解析为 [`Config`]。
// Config 结构体定义在 usecases::config；本模块只负责 IO + 解析。
use std::env;
use std::fs;
use std::sync::OnceLock;

use tracing::debug;
use tracing::warn;

use crate::usecases::config::Config;

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
            cfg
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

fn find_close_brace(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .iter()
        .position(|&b| b == b'}')
        .map(|off| start + off)
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

    #[test]
    fn resolve_var_missing_no_default_returns_empty() {
        unsafe { std::env::remove_var("TIDYMEDIA_TEST_NO_DEFAULT_W") };
        assert_eq!(resolve_var("TIDYMEDIA_TEST_NO_DEFAULT_W"), "");
    }

    #[test]
    fn backend_config_yaml_overrides_defaults() {
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
        assert_eq!(cfg.backend.smb.timeout_secs, 60);
        assert_eq!(cfg.backend.mtp.device_match, "exact");
        assert_eq!(cfg.backend.mtp.storage_match, "exact");
        assert_eq!(cfg.backend.adb.server_host, "10.0.0.5");
        assert_eq!(cfg.backend.adb.server_port, 15037);
        assert_eq!(cfg.backend.adb.timeout_secs, 90);
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
}
