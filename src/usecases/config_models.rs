// 运行时配置子结构体 + Default 默认值。拆自 config.rs（原 352 行 → ≤300）；
// 主结构体 [`Config`]、OnceLock 与全局访问器在 config.rs，本模块只声明数据结构。
use serde_derive::Deserialize;

use super::{DEFAULT_ARCHIVE_TEMPLATE, DEFAULT_DOC_ARCHIVE_TEMPLATE};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CopyConfig {
    pub timezone_offset_hours: i8,
    pub unique_name_max_attempts: u32,
    pub archive_template: String,
    /// `copy-doc`/`move-doc` 未传 `--archive-template` 时的默认模板
    /// （文档按内容类目分桶，时间在内层）。
    pub doc_archive_template: String,
    pub skip_extensions: Vec<String>,
}

impl Default for CopyConfig {
    fn default() -> Self {
        Self {
            timezone_offset_hours: 8,
            unique_name_max_attempts: 10,
            archive_template: DEFAULT_ARCHIVE_TEMPLATE.to_string(),
            doc_archive_template: DEFAULT_DOC_ARCHIVE_TEMPLATE.to_string(),
            skip_extensions: ["js", "json", "svg", "css", "ts", "html", "xml", "map"]
                .map(str::to_owned)
                .to_vec(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VerifyConfig {
    pub max_image_bytes: u64,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            max_image_bytes: 50 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct SmbBackendConfig {
    pub default_user: String,
    pub workgroup: String,
    /// smb2 `ClientConfig.timeout`（连接/单次请求超时秒数）；0 非法，
    /// sanitize 回退默认 30。
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

/// `move-text-shot` 子命令的文本检测后端参数。模型文件路径外置；
/// 二值化与「响应像素占比」两阈值都暴露让用户按机型/语言调优。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrConfig {
    /// `PaddleOCR` `DBNet` `det.onnx` 模型本地路径。空串 = feature on 调用时报 `InvalidInput`。
    pub det_model_path: String,
    /// sigmoid 输出图二值化阈值（DBNet 训练惯用 0.3）。
    pub binarize_threshold: f32,
    /// 「二值化后前景像素 / 总像素」比例下限；高于此值视为含文本。
    pub min_text_pixel_ratio: f32,
    /// 推理前 resize 的短边像素上限；DBNet 要求 32 倍数（实际 resize 时按 32 对齐）。
    pub resize_max_side: u32,
    /// 单文件字节上限；超过此值的图 walk 阶段直接 skip 计入 `skipped_too_large`（防 OOM）。
    /// 默认 50 MiB，覆盖典型手机/相机截图 + 适度裕度；文档扫描类大图需自行调高。
    pub max_image_bytes: u64,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            det_model_path: String::new(),
            binarize_threshold: 0.3,
            min_text_pixel_ratio: 0.005,
            resize_max_side: 736,
            max_image_bytes: 50 * 1024 * 1024,
        }
    }
}

/// `copy-doc`/`move-doc` 内容分类的单个用户类目：`name` 进 `{category}` 归档
/// 路径段，`description` 是 zero-shot 原型文本（embedding 后与文档正文比相似度）。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CategoryDef {
    pub name: String,
    pub description: String,
}

/// `copy-doc`/`move-doc` 内容分类后端参数。模型/tokenizer 路径外置；
/// `categories` 用户可自定义类目集（空 = 全部落 uncategorized，不加载模型）。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClassifyConfig {
    /// bge-small-zh embedding ONNX 本地路径。空串 = 调用时报 `InvalidInput`。
    pub embed_model_path: String,
    /// 模型配套 `tokenizer.json` 本地路径。空串同上。
    pub tokenizer_path: String,
    /// 用户类目集；顺序无关（取 cosine argmax）。
    pub categories: Vec<CategoryDef>,
    /// cosine 相似度下限；低于此值落 uncategorized。spike 实测命中类目
    /// 0.60–0.81、无关文本最高 0.36，默认 0.5 两侧留余量。
    pub score_min: f32,
    /// 提取给分类器的正文文本字节上限（embedding 只吃前几百 token）。
    pub max_text_bytes: usize,
}

impl Default for ClassifyConfig {
    fn default() -> Self {
        Self {
            embed_model_path: String::new(),
            tokenizer_path: String::new(),
            categories: Vec::new(),
            score_min: 0.5,
            max_text_bytes: 4096,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackendConfig {
    pub smb: SmbBackendConfig,
    pub ocr: OcrConfig,
    pub classify: ClassifyConfig,
}
