// 运行时配置子结构体 + Default 默认值。拆自 config.rs（原 352 行 → ≤300）；
// 主结构体 [`Config`]、OnceLock 与全局访问器在 config.rs，本模块只声明数据结构。
use serde_derive::Deserialize;

use super::{DEFAULT_ARCHIVE_TEMPLATE, DEFAULT_DOC_ARCHIVE_TEMPLATE};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CopyConfig {
    pub timezone_offset_hours: i8,
    pub unique_name_max_attempts: u32,
    pub archive_template: String,
    /// `copy-doc`/`move-doc` 未传 `--archive-template` 时的默认模板
    /// （文档按内容类目分桶，时间在内层）。
    pub doc_archive_template: String,
}

impl Default for CopyConfig {
    fn default() -> Self {
        Self {
            timezone_offset_hours: 8,
            unique_name_max_attempts: 10,
            archive_template: DEFAULT_ARCHIVE_TEMPLATE.to_string(),
            doc_archive_template: DEFAULT_DOC_ARCHIVE_TEMPLATE.to_string(),
        }
    }
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
#[serde(default)]
pub struct CategoryDef {
    pub name: String,
    pub description: String,
}

/// `copy-doc`/`move-doc` 内容分类后端参数。模型/tokenizer 路径外置；
/// `categories` 用户可自定义类目集（空 = 全部落 uncategorized，不加载模型）。
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
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
    pub classify: ClassifyConfig,
}
