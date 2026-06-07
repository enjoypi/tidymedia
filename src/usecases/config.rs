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

/// 校验归档模板：非空 + `{` `}` 结构配对 + 占位符名属已知集合。
///
/// 结构扫描替代旧的字符计数：`{year/{month}}` 计数配平但渲染时占位符无法
/// 整 token 匹配，会静默产生字面 `{year` 目录；未知占位符（如 `{foo}`）同理。
///
/// # Errors
///
/// 模板为空、花括号嵌套/错配/未闭合、或占位符名未知时返回 `Err`。
pub fn validate_archive_template(template: &str) -> Result<(), String> {
    if template.is_empty() {
        return Err("archive_template must not be empty".into());
    }
    let mut start: Option<usize> = None;
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
            }
            _ => {}
        }
    }
    if start.is_some() {
        return Err("archive_template has unbalanced braces: unclosed '{'".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// 默认日志级别（trace/debug/info/warn/error）；CLI `--log-level` 与
    /// `RUST_LOG` 均优先于此值。
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
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

// 哑配置治理（杜绝声明了却无消费点的字段）：
// - `smb.timeout_secs` / `adb.timeout_secs` 已删——pavao `SmbOptions` 与 adb_client
//   均无 timeout API，字段只会制造"配置了却无效"的幻觉；库支持后再加回
// - `MtpBackendConfig`（device_match / storage_match）已删——MTP real client 是
//   stub，factory 不读这两个字段；real 接入时随 `MtpMatch` 消费链一起加回
// serde 默认忽略未知字段，旧 config.yaml 含这些键不会报错。
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SmbBackendConfig {
    pub default_user: String,
    pub workgroup: String,
}

impl Default for SmbBackendConfig {
    fn default() -> Self {
        Self {
            default_user: String::new(),
            workgroup: "WORKGROUP".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct AdbBackendConfig {
    pub server_host: String,
    pub server_port: u16,
}

impl Default for AdbBackendConfig {
    fn default() -> Self {
        Self {
            server_host: "127.0.0.1".into(),
            server_port: 5037,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BackendConfig {
    pub smb: SmbBackendConfig,
    pub adb: AdbBackendConfig,
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
        assert_eq!(c.backend.adb.server_host, "127.0.0.1");
        assert_eq!(c.backend.adb.server_port, 5037);
        assert_eq!(c.log.level, "info");
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

    // 计数配平但结构错配：旧字符计数实现会放过，渲染时产生字面 '{year' 目录。
    #[test]
    fn validate_archive_template_rejects_count_balanced_but_nested() {
        let err = validate_archive_template("{year/{month}}").unwrap_err();
        assert!(err.contains("nested"), "got: {err}");
    }

    #[test]
    fn validate_archive_template_rejects_unclosed_open() {
        let err = validate_archive_template("{year").unwrap_err();
        assert!(err.contains("unclosed"), "got: {err}");
    }

    #[test]
    fn validate_archive_template_rejects_unknown_placeholder() {
        let err = validate_archive_template("{year}/{foo}").unwrap_err();
        assert!(err.contains("unknown placeholder {foo}"), "got: {err}");
    }

    #[test]
    fn validate_archive_template_accepts_all_known_placeholders() {
        assert!(
            validate_archive_template("{year}/{month}/{day}/{make}/{model}/{valuable_name}")
                .is_ok()
        );
    }
}
