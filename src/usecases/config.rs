// 运行时配置结构体定义 + 默认值。
// 解析顺序：硬编码默认值 -> config.yaml(若存在) -> 环境变量替换 `${VAR:-default}`。
// IO 加载逻辑（load/expand_env/config 全局访问器）在 frameworks::config。

// Re-export: usecases 层内部代码（copy.rs 等）通过此路径获取全局配置实例，
// 避免直接依赖 frameworks 层（依赖方向 usecases → entities，不 → frameworks）。
pub use crate::frameworks::config::config;
use serde_derive::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CopyConfig {
    pub timezone_offset_hours: i8,
    pub unique_name_max_attempts: u32,
}

impl Default for CopyConfig {
    fn default() -> Self {
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
    use super::Config;

    #[test]
    fn config_defaults_match_historical_constants() {
        let c = Config::default();
        assert_eq!(c.copy.timezone_offset_hours, 8);
        assert_eq!(c.copy.unique_name_max_attempts, 10);
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
}
