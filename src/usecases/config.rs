// 运行时配置结构体定义 + 默认值 + 全局访问器。
// 解析顺序：硬编码默认值 -> config.yaml(若存在) -> 环境变量替换 `${VAR:-default}`。
// yaml IO + sanitize 实现在 frameworks::config，按依赖倒置注入：frameworks 启动
// 时调 `frameworks::config::install_global_loader()` 把 `load` fn 装到本模块的
// [`LOADER`]；lazy init 命中时取出执行。usecases 不直接依赖 frameworks。

use std::sync::OnceLock;

use serde_derive::Deserialize;

static CONFIG: OnceLock<Config> = OnceLock::new();
static LOADER: OnceLock<fn() -> Config> = OnceLock::new();

/// 全局只读配置；首次访问时取 [`LOADER`] 加载，未装则用 [`Config::default`]。
/// CLI / FFI 启动期 MUST 先调 `crate::install_config_loader()`，否则用户的
/// yaml 不会生效。
pub fn config() -> &'static Config {
    CONFIG.get_or_init(|| LOADER.get().map_or_else(Config::default, |f| f()))
}

/// 一次性注入加载器（fn pointer）；frameworks 层调用，依赖倒置入口。
/// 多次调用静默忽略后续（OnceLock 语义）。
pub fn install_loader(loader: fn() -> Config) {
    let _ = LOADER.set(loader);
}

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
    // 保证渲染后至少有一段非空目录的占位符——剩下的 {valuable_name} 可能渲染为
    // 空串致全部文件落 output 根；要求模板含至少一个 always-non-empty 占位符。
    // const 须放 fn 顶（clippy::items_after_statements，pedantic）。
    const ALWAYS_NON_EMPTY: [&str; 5] = ["year", "month", "day", "make", "model"];
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

/// `move-text-shot` 子命令的文本检测后端参数。模型文件路径外置；
/// 二值化与「响应像素占比」两阈值都暴露让用户按机型/语言调优。
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct OcrConfig {
    /// `PaddleOCR` `DBNet` `det.onnx` 模型本地路径。空串 = feature on 调用时报 `InvalidInput`。
    pub det_model_path: String,
    /// sigmoid 输出图二值化阈值（DBNet 训练惯用 0.3）。
    pub binarize_threshold: f32,
    /// 「二值化后前景像素 / 总像素」比例下限；高于此值视为含文本。
    pub min_text_pixel_ratio: f32,
    /// 推理前 resize 的短边像素上限；DBNet 要求 32 倍数（实际 resize 时按 32 对齐）。
    pub resize_max_side: u32,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            det_model_path: String::new(),
            binarize_threshold: 0.3,
            min_text_pixel_ratio: 0.005,
            resize_max_side: 736,
        }
    }
}

/// `cull` 子命令的人脸质量评分参数：4 个 ONNX 模型路径 + pHash/清晰度/EAR
/// 阈值 + 综合评分权重。模型不入 git，路径外置；阈值与权重暴露让用户按场景调优。
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct FaceConfig {
    /// SCRFD-10G-bn-kps（人脸 bbox + 5 点关键点；antelopev2 默认变体）ONNX 路径；
    /// 空 = 调用时报 `InvalidInput`。
    pub scrfd_model_path: String,
    /// SCRFD 检测置信度阈值；低于此值的 anchor 丢弃。范围 `(0, 1)`。
    /// 光线差 / 模型变体需调；合影密集脸场景可调低到 0.3。
    pub scrfd_score_threshold: f32,
    /// SCRFD 非极大值抑制 `IoU` 阈值；超此值的重叠 bbox 折叠为一个。范围 `(0, 1)`。
    /// 密集脸场景调 0.3 增强去重；稀疏场景默认 0.4。
    pub scrfd_nms_iou: f32,
    /// `MobileFaceNet`（112×112 → 128 维 embedding；foamliu/MobileFaceNet 训练规格）ONNX 路径。
    /// 切换 512 维变体需同步改 `EMBED_DIM` 与 `FaceEmbedder` trait 接口。
    pub facenet_model_path: String,
    /// `MediaPipe` `FaceMesh`（468 点 3D 关键点；`PINTO_model_zoo` 静态 192×192 版）ONNX 路径。
    pub facemesh_model_path: String,
    /// `YOLOv8` `EyeState`（640×640 letterbox → 检测头 max conf；
    /// `MichalMlodawski/open-closed-eye-detection`）ONNX 路径。
    pub eyestate_model_path: String,
    /// pHash 汉明距离阈值；≤ 此值的两图视为相似入同组。范围 `[1, 64]`。
    pub phash_hamming_max: u8,
    /// 全图 Laplacian 方差下限；低于此值视整图模糊丢弃（单图组例外保留）。
    pub sharpness_min: f32,
    /// 跨图人脸 embedding 余弦相似度阈值；≥ 此值判同一身份。范围 `(0, 1)`。
    /// 当前仅 `cluster_identities` 产 debug 日志消费，`pick_best_for_group` 不消费
    /// cluster 结果；未来 per-identity 策略接入后此值才影响 best 选择（TODO）。
    pub face_cosine_min: f32,
    /// EAR（眼睑纵横比）阈值；低于此值视为闭眼。范围 `(0, 1)`。
    pub ear_blink_max: f32,
    /// `EyeState` 模型闭眼概率阈值。范围 `(0, 1)`。
    pub eye_blink_score_max: f32,
    /// 眼部 crop 半径相对人脸 bbox 高度的比例；左/右眼各按此半径方形 crop 喂 `EyeState`。
    /// 范围 `(0, 1)`。儿童 / 老人脸型或宽距镜头可调 0.12~0.15 扩大感受野。
    pub eye_crop_radius_ratio: f32,
    /// 综合评分中清晰度权重。
    pub w_sharpness: f32,
    /// 综合评分中闭眼惩罚权重。
    pub w_blink: f32,
    /// 综合评分中微笑加分权重。
    pub w_smile: f32,
    /// 单文件字节上限；超过此值的图扫描阶段直接 skip 计入 `failed`（防 OOM）。
    /// 默认 50 MiB，覆盖典型相机原始 JPEG/HEIC + 适度裕度；大 RAW 文件需自行调高。
    pub max_image_bytes: u64,
}

impl Default for FaceConfig {
    fn default() -> Self {
        Self {
            scrfd_model_path: String::new(),
            scrfd_score_threshold: 0.5,
            scrfd_nms_iou: 0.4,
            facenet_model_path: String::new(),
            facemesh_model_path: String::new(),
            eyestate_model_path: String::new(),
            phash_hamming_max: 10,
            sharpness_min: 100.0,
            face_cosine_min: 0.5,
            ear_blink_max: 0.21,
            eye_blink_score_max: 0.5,
            eye_crop_radius_ratio: 0.10,
            w_sharpness: 1.0,
            w_blink: 2.0,
            w_smile: 0.5,
            max_image_bytes: 50 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BackendConfig {
    pub smb: SmbBackendConfig,
    pub adb: AdbBackendConfig,
    pub ocr: OcrConfig,
    pub face: FaceConfig,
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
        assert_eq!(c.backend.ocr.det_model_path, "");
        assert!((c.backend.ocr.binarize_threshold - 0.3).abs() < f32::EPSILON);
        assert!((c.backend.ocr.min_text_pixel_ratio - 0.005).abs() < f32::EPSILON);
        assert_eq!(c.backend.ocr.resize_max_side, 736);
        assert_eq!(c.backend.face.scrfd_model_path, "");
        assert!((c.backend.face.scrfd_score_threshold - 0.5).abs() < f32::EPSILON);
        assert!((c.backend.face.scrfd_nms_iou - 0.4).abs() < f32::EPSILON);
        assert_eq!(c.backend.face.facenet_model_path, "");
        assert_eq!(c.backend.face.facemesh_model_path, "");
        assert_eq!(c.backend.face.eyestate_model_path, "");
        assert_eq!(c.backend.face.phash_hamming_max, 10);
        assert!((c.backend.face.sharpness_min - 100.0).abs() < f32::EPSILON);
        assert!((c.backend.face.face_cosine_min - 0.5).abs() < f32::EPSILON);
        assert!((c.backend.face.ear_blink_max - 0.21).abs() < f32::EPSILON);
        assert!((c.backend.face.eye_blink_score_max - 0.5).abs() < f32::EPSILON);
        assert!((c.backend.face.eye_crop_radius_ratio - 0.10).abs() < f32::EPSILON);
        assert!((c.backend.face.w_sharpness - 1.0).abs() < f32::EPSILON);
        assert!((c.backend.face.w_blink - 2.0).abs() < f32::EPSILON);
        assert!((c.backend.face.w_smile - 0.5).abs() < f32::EPSILON);
        assert_eq!(c.backend.face.max_image_bytes, 50 * 1024 * 1024);
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

    /// 仅 `{valuable_name}` 单占位符 → 渲染时 `valuable_name` 可能空 → 文件全落
    /// output 根；validate 必须显式拒绝缺乏 always-non-empty 占位符的模板。
    #[test]
    fn validate_archive_template_rejects_template_without_safe_placeholder() {
        let err = validate_archive_template("{valuable_name}").unwrap_err();
        assert!(
            err.contains("at least one of"),
            "expected guidance about safe placeholders, got: {err}"
        );
    }

    /// 单 always-non-empty 占位符即可通过（year 已保证非空）。
    #[test]
    fn validate_archive_template_accepts_single_safe_placeholder() {
        assert!(validate_archive_template("{year}").is_ok());
    }

    /// 纯静态文本无占位符 → 渲染恒同字面量，无年/月分桶，同样拒绝。
    #[test]
    fn validate_archive_template_rejects_pure_static_text() {
        let err = validate_archive_template("archive").unwrap_err();
        assert!(err.contains("at least one of"), "got: {err}");
    }
}
