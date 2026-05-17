// 运行时配置加载。
// 解析顺序：硬编码默认值 -> config.yaml(若存在) -> 环境变量替换 `${VAR:-default}`。
// 单文件 CLI 工具不需要复杂层级，扁平结构即可。
use std::env;
use std::fs;
use std::sync::OnceLock;

use serde_derive::Deserialize;
use tracing::debug;
use tracing::warn;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CopyConfig {
    pub timezone_offset_hours: i8,
    pub unique_name_max_attempts: u32,
}

impl Default for CopyConfig {
    fn default() -> Self {
        // 默认与历史行为保持一致：北京时间 +8，重名尝试 10 次
        Self {
            timezone_offset_hours: 8,
            unique_name_max_attempts: 10,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ExifConfig {
    pub valid_date_time_secs: u64,
}

impl Default for ExifConfig {
    fn default() -> Self {
        // 2000-01-01T00:00:00Z，对应原 VALID_DATE_TIME
        Self { valid_date_time_secs: 946_684_800 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SmbBackendConfig {
    /// URI 内未写 `user@` 时兜底；密码总是经环境变量 `SMB_PASSWORD` 注入
    pub default_user: String,
    /// 单次 SMB 操作超时秒数；真实 client 接入后生效
    pub timeout_secs: u64,
}

impl Default for SmbBackendConfig {
    fn default() -> Self {
        Self {
            default_user: String::new(),
            timeout_secs: 30,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MtpBackendConfig {
    /// `exact` 或 `fuzzy`；fuzzy 由 client 自决（通常走 contains）
    pub device_match: String,
    pub storage_match: String,
}

impl Default for MtpBackendConfig {
    fn default() -> Self {
        Self {
            device_match: "fuzzy".into(),
            storage_match: "fuzzy".into(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BackendConfig {
    pub smb: SmbBackendConfig,
    pub mtp: MtpBackendConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub copy: CopyConfig,
    pub exif: ExifConfig,
    pub backend: BackendConfig,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// 全局只读配置；首次调用时加载。
pub fn config() -> &'static Config {
    CONFIG.get_or_init(load)
}

fn load() -> Config {
    let path = env::var("TIDYMEDIA_CONFIG")
        .unwrap_or_else(|_| "config.yaml".to_string());

    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            // 文件缺失不致命：CLI 工具应能裸跑
            debug!(
                feature = "config",
                operation = "load",
                result = "fallback_default",
                path = %path,
                "config file missing, using defaults"
            );
            return Config::default();
        }
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
/// 不支持嵌套、不支持转义——CLI 配置场景足够。
fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            if let Some(end) = find_close_brace(bytes, i + 2) {
                let body = &input[i + 2..end];
                out.push_str(&resolve_var(body));
                i = end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_close_brace(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .iter()
        .position(|&b| b == b'}')
        .map(|off| start + off)
}

fn resolve_var(body: &str) -> String {
    // body 形如 `NAME` 或 `NAME:-default`
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
        std::env::remove_var("TIDYMEDIA_TEST_MISSING_VAR_X");
        let s = expand_env("a: ${TIDYMEDIA_TEST_MISSING_VAR_X:-7}");
        assert_eq!(s, "a: 7");
    }

    #[test]
    fn expand_env_uses_env_value_when_set() {
        std::env::set_var("TIDYMEDIA_TEST_SET_VAR_Y", "42");
        let s = expand_env("a: ${TIDYMEDIA_TEST_SET_VAR_Y:-0}");
        assert_eq!(s, "a: 42");
        std::env::remove_var("TIDYMEDIA_TEST_SET_VAR_Y");
    }

    #[test]
    fn expand_env_resolves_bare_name_without_default() {
        std::env::set_var("TIDYMEDIA_TEST_BARE_Z", "hi");
        let s = expand_env("k: ${TIDYMEDIA_TEST_BARE_Z}");
        assert_eq!(s, "k: hi");
        std::env::remove_var("TIDYMEDIA_TEST_BARE_Z");
    }

    #[test]
    fn expand_env_keeps_text_without_placeholder() {
        assert_eq!(expand_env("plain: text"), "plain: text");
    }

    #[test]
    fn expand_env_leaves_unterminated_brace() {
        // 不闭合的 ${ 序列保持原样
        assert_eq!(expand_env("a: ${UNCLOSED"), "a: ${UNCLOSED");
    }

    #[test]
    fn expand_env_handles_trailing_dollar() {
        assert_eq!(expand_env("a$"), "a$");
    }

    #[test]
    fn resolve_var_missing_no_default_returns_empty() {
        std::env::remove_var("TIDYMEDIA_TEST_NO_DEFAULT_W");
        assert_eq!(resolve_var("TIDYMEDIA_TEST_NO_DEFAULT_W"), "");
    }

    #[test]
    fn config_defaults_match_historical_constants() {
        let c = Config::default();
        assert_eq!(c.copy.timezone_offset_hours, 8);
        assert_eq!(c.copy.unique_name_max_attempts, 10);
        assert_eq!(c.exif.valid_date_time_secs, 946_684_800);
        assert_eq!(c.backend.smb.default_user, "");
        assert_eq!(c.backend.smb.timeout_secs, 30);
        assert_eq!(c.backend.mtp.device_match, "fuzzy");
        assert_eq!(c.backend.mtp.storage_match, "fuzzy");
    }

    #[test]
    fn backend_config_yaml_overrides_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.yaml");
        std::fs::write(
            &path,
            "backend:\n  smb:\n    default_user: alice\n    timeout_secs: 60\n  mtp:\n    device_match: exact\n    storage_match: exact\n",
        )
        .unwrap();
        std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
        let cfg = load();
        assert_eq!(cfg.backend.smb.default_user, "alice");
        assert_eq!(cfg.backend.smb.timeout_secs, 60);
        assert_eq!(cfg.backend.mtp.device_match, "exact");
        assert_eq!(cfg.backend.mtp.storage_match, "exact");
        std::env::remove_var("TIDYMEDIA_CONFIG");
    }

    #[test]
    fn load_falls_back_when_file_missing() {
        std::env::set_var("TIDYMEDIA_CONFIG", "/no/such/file/xyz.yaml");
        let cfg = load();
        assert_eq!(cfg.copy.timezone_offset_hours, 8);
        std::env::remove_var("TIDYMEDIA_CONFIG");
    }

    #[test]
    fn load_falls_back_when_yaml_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.yaml");
        std::fs::write(&path, "::: not yaml :::").unwrap();
        std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
        let cfg = load();
        assert_eq!(cfg.copy.unique_name_max_attempts, 10);
        std::env::remove_var("TIDYMEDIA_CONFIG");
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
        std::env::set_var("TIDYMEDIA_CONFIG", path.to_str().unwrap());
        let cfg = load();
        assert_eq!(cfg.copy.timezone_offset_hours, 0);
        assert_eq!(cfg.copy.unique_name_max_attempts, 5);
        assert_eq!(cfg.exif.valid_date_time_secs, 100);
        std::env::remove_var("TIDYMEDIA_CONFIG");
    }

    #[test]
    fn config_global_accessor_returns_static() {
        let a = config();
        let b = config();
        assert!(std::ptr::eq(a, b));
    }
}
