// 运行时配置结构体定义 + 默认值。
// 解析顺序：硬编码默认值 -> config.yaml(若存在) -> 环境变量替换 `${VAR:-default}`。
// IO 加载逻辑（load/expand_env/config 全局访问器）在 frameworks::config。

// Re-export: usecases 层内部代码（copy.rs 等）通过此路径获取全局配置实例，
// 避免直接依赖 frameworks 层（依赖方向 usecases → entities，不 → frameworks）。
pub use crate::frameworks::config::config;
use serde_derive::Deserialize;

/// 默认归档模板：`{year}/{month}/{valuable_name}`。
/// `{valuable_name}` 为路径中首个含非 ASCII 的目录段；若不存在则该段为空串。
pub const DEFAULT_ARCHIVE_TEMPLATE: &str = "{year}/{month}/{valuable_name}";

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CopyConfig {
    pub timezone_offset_hours: i8,
    pub unique_name_max_attempts: u32,
    pub archive_template: String,
}

impl Default for CopyConfig {
    fn default() -> Self {
        Self {
            timezone_offset_hours: 8,
            unique_name_max_attempts: 10,
            archive_template: DEFAULT_ARCHIVE_TEMPLATE.to_string(),
        }
    }
}

/// 校验归档模板：非空 + `{` `}` 配对（简单字符计数）。
///
/// # Errors
///
/// 模板为空或花括号不配对时返回 `Err`。
pub fn validate_archive_template(template: &str) -> Result<(), String> {
    if template.is_empty() {
        return Err("archive_template must not be empty".into());
    }
    let open = template.chars().filter(|&c| c == '{').count();
    let close = template.chars().filter(|&c| c == '}').count();
    if open != close {
        return Err(format!(
            "archive_template has unbalanced braces: {open} '{{' vs {close} '}}'"
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ExifConfig {
    pub valid_date_time_secs: u64,
}

impl Default for ExifConfig {
    fn default() -> Self {
        Self {
            valid_date_time_secs: 946_684_800,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SmbBackendConfig {
    pub default_user: String,
    pub workgroup: String,
    pub timeout_secs: u64,
}

impl Default for SmbBackendConfig {
    fn default() -> Self {
        Self {
            default_user: String::new(),
            workgroup: "WORKGROUP".into(),
            timeout_secs: 30,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MtpBackendConfig {
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

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct AdbBackendConfig {
    pub server_host: String,
    pub server_port: u16,
    pub timeout_secs: u64,
}

impl Default for AdbBackendConfig {
    fn default() -> Self {
        Self {
            server_host: "127.0.0.1".into(),
            server_port: 5037,
            timeout_secs: 30,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BackendConfig {
    pub smb: SmbBackendConfig,
    pub mtp: MtpBackendConfig,
    pub adb: AdbBackendConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub copy: CopyConfig,
    pub exif: ExifConfig,
    pub backend: BackendConfig,
}

#[cfg(test)]
mod tests {
    use super::{Config, validate_archive_template};

    #[test]
    fn config_defaults_match_historical_constants() {
        let c = Config::default();
        assert_eq!(c.copy.timezone_offset_hours, 8);
        assert_eq!(c.copy.unique_name_max_attempts, 10);
        assert_eq!(c.copy.archive_template, "{year}/{month}/{valuable_name}");
        assert_eq!(c.exif.valid_date_time_secs, 946_684_800);
        assert_eq!(c.backend.smb.default_user, "");
        assert_eq!(c.backend.smb.workgroup, "WORKGROUP");
        assert_eq!(c.backend.smb.timeout_secs, 30);
        assert_eq!(c.backend.mtp.device_match, "fuzzy");
        assert_eq!(c.backend.mtp.storage_match, "fuzzy");
        assert_eq!(c.backend.adb.server_host, "127.0.0.1");
        assert_eq!(c.backend.adb.server_port, 5037);
        assert_eq!(c.backend.adb.timeout_secs, 30);
    }

    #[test]
    fn validate_archive_template_accepts_valid_template() {
        assert!(validate_archive_template("{year}/{month}/{day}").is_ok());
    }

    #[test]
    fn validate_archive_template_rejects_empty() {
        assert!(validate_archive_template("").is_err());
    }

    #[test]
    fn validate_archive_template_rejects_unbalanced_open() {
        let err = validate_archive_template("{year/{month}").unwrap_err();
        assert!(err.contains("unbalanced"), "got: {err}");
    }

    #[test]
    fn validate_archive_template_rejects_unbalanced_close() {
        let err = validate_archive_template("year}/month").unwrap_err();
        assert!(err.contains("unbalanced"), "got: {err}");
    }
}
