// 配置值 sanitize：非法字段值回退默认并告警。与「parse 失败回退 `Config::default`」
// 同一哲学：配置错误不让 CLI panic 或静默全量失败，但必须可观测。
// 拆自 `config.rs`（原 518 行 → 骨架 + sanitize + expand_env 三文件），FaceConfig
// 校验族再拆 `config_sanitize_face.rs`。
use tracing::warn;

use crate::usecases::config::{
    ClassifyConfig, Config, CopyConfig, LogConfig, OcrConfig, validate_archive_template,
};

#[path = "config_sanitize_face.rs"]
mod face;

/// 非法字段值回退默认并告警，与"parse 失败回退 `Config::default`"同一哲学：
/// 配置错误不让 CLI panic 或静默全量失败，但必须可观测。
/// - `unique_name_max_attempts == 0` 会让 `generate_unique_name` 的 `0..0` 循环
///   永不执行恒返 `None`，所有 copy/move 静默失败
/// - 非法 `archive_template`（嵌套/错配/未知占位符）会渲染出字面 `{xxx}` 目录
pub fn sanitize(mut cfg: Config) -> Config {
    // `copy.timezone_offset_hours` 上限：chrono::FixedOffset::east_opt 限 ±24h-1s、
    // time::UtcOffset::from_whole_seconds 限 ±25:59:59。统一收紧到 ±23 给两库都留
    // buffer；超界让 offset_from_hours / chrono_offset_from_hours 静默回退 UTC，
    // 月末文件跨月归错桶，必须 warn + 回退默认。const 内联避免顶层 const 在
    // multi-binary instance 下被 LLVM 单独计 region。
    const MAX_TIMEZONE_HOURS_ABS: u8 = 23;
    if cfg.copy.timezone_offset_hours.unsigned_abs() > MAX_TIMEZONE_HOURS_ABS {
        let fallback = CopyConfig::default().timezone_offset_hours;
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "copy.timezone_offset_hours",
            value = cfg.copy.timezone_offset_hours,
            fallback,
            "timezone_offset_hours must be within ±23; falling back to default"
        );
        cfg.copy.timezone_offset_hours = fallback;
    }
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
    sanitize_template_field(
        &mut cfg.copy.archive_template,
        CopyConfig::default().archive_template,
        "copy.archive_template",
    );
    sanitize_template_field(
        &mut cfg.copy.doc_archive_template,
        CopyConfig::default().doc_archive_template,
        "copy.doc_archive_template",
    );
    sanitize_ocr(&mut cfg.backend.ocr);
    face::sanitize_face(&mut cfg.backend.face);
    sanitize_classify(&mut cfg.backend.classify);
    // 0 让 smb2 每次请求立即超时，归档全量失败且报错指向超时而非配置。
    if cfg.backend.smb.timeout_secs == 0 {
        let fallback = crate::usecases::config::SmbBackendConfig::default().timeout_secs;
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.smb.timeout_secs",
            value = cfg.backend.smb.timeout_secs,
            fallback,
            "smb timeout_secs must be >= 1; falling back to default"
        );
        cfg.backend.smb.timeout_secs = fallback;
    }
    // 非法 level 会让 CLI 端 parse 失败静默退 info；此处统一回退 + 告警。
    // sanitize 在 install_logging 之前由 OnceLock lazy init 触发 → tracing subscriber
    // 尚未安装，`warn!` 投到默认 no-op dispatcher 被丢弃；user 看不到 fallback。
    // 用 `eprintln!` 兜底直接走 stderr 保证可见性（user 端边界异常态，结构化日志
    // 缺失换 user 可见性是合理 trade-off）。
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
        eprintln_sanitize_fallback("log.level", &cfg.log.level, &fallback);
        cfg.log.level = fallback;
    }
    cfg
}

/// sanitize 前期发生在 `install_logging` 之前 → tracing subscriber 未装让 `warn!` 不可见。
/// stderr 直写兜底保证 user 看到 fallback。**仅用于 config sanitize 路径**：业务热路径
/// 仍走 tracing！。
pub(super) fn eprintln_sanitize_fallback(
    field: &str,
    value: &str,
    fallback: &dyn std::fmt::Display,
) {
    eprintln!("tidymedia: config {field}={value} invalid; falling back to {fallback}");
}

// archive_template / doc_archive_template 共用的模板校验回退：非法模板会渲染出
// 字面 `{xxx}` 目录，warn + stderr 兜底 + 回退默认（关键 fallback 双通道可见）。
fn sanitize_template_field(value: &mut String, fallback: String, field: &'static str) {
    if let Err(e) = validate_archive_template(value) {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field,
            error = %e,
            fallback = %fallback,
            "archive template invalid; falling back to default"
        );
        eprintln_sanitize_fallback(field, &format!("invalid: {e}"), &fallback);
        *value = fallback;
    }
}

// OCR 三阈值非法即 warn + 回退默认；与 `archive_template` 同哲学（feature off
// 时仍走此校验，让 yaml 内字段格式问题统一可观测）。
// - `binarize_threshold ∈ (0, 1)`：DBNet sigmoid 输出域，越界即恒真/恒假
// - `min_text_pixel_ratio ∈ (0, 1)`：占比阈值，越界让所有图都判命中或永不命中
// - `resize_max_side >= 64`：太小让 DBNet 输入丢失结构信息
fn sanitize_ocr(ocr: &mut OcrConfig) {
    // 顶置常量：clippy::items_after_statements 禁止 statement 后插 const/fn
    const MIN_RESIZE_SIDE: u32 = 64;

    let defaults = OcrConfig::default();
    if !is_unit_open(ocr.binarize_threshold) {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.ocr.binarize_threshold",
            value = ocr.binarize_threshold,
            fallback = defaults.binarize_threshold,
            "binarize_threshold must be in (0, 1); falling back to default"
        );
        ocr.binarize_threshold = defaults.binarize_threshold;
    }
    if !is_unit_open(ocr.min_text_pixel_ratio) {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.ocr.min_text_pixel_ratio",
            value = ocr.min_text_pixel_ratio,
            fallback = defaults.min_text_pixel_ratio,
            "min_text_pixel_ratio must be in (0, 1); falling back to default"
        );
        ocr.min_text_pixel_ratio = defaults.min_text_pixel_ratio;
    }
    if ocr.resize_max_side < MIN_RESIZE_SIDE {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.ocr.resize_max_side",
            value = ocr.resize_max_side,
            fallback = defaults.resize_max_side,
            "resize_max_side must be >= 64; falling back to default"
        );
        ocr.resize_max_side = defaults.resize_max_side;
    }
    sanitize_max_image_bytes_field(
        &mut ocr.max_image_bytes,
        defaults.max_image_bytes,
        "backend.ocr.max_image_bytes",
    );
}

// 开区间 (0.0, 1.0) 内的有限正数。NaN/Inf 均通过 `is_finite()` 拒绝。
fn is_unit_open(v: f32) -> bool {
    if !v.is_finite() {
        return false;
    }
    if v <= 0.0 {
        return false;
    }
    v < 1.0
}

// max_image_bytes 太小会让所有图都被判超限跳过整个 pipeline；
// 1 MiB 以下没有业务场景（JPEG 缩略图都 > 100 KiB），统一收紧到 ≥ 1 MiB。
// ocr 与 face 共用同一门限：max_image_bytes 语义（防单文件 OOM）与业务无关。
fn sanitize_max_image_bytes_field(value: &mut u64, fallback: u64, field: &str) {
    const MIN_IMAGE_BYTES: u64 = 1024 * 1024;
    if *value < MIN_IMAGE_BYTES {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field,
            value = *value,
            fallback,
            "max_image_bytes must be >= 1 MiB; falling back to default"
        );
        *value = fallback;
    }
}

// classify 两参数非法即 warn + 回退默认，与 sanitize_ocr 同哲学：
// - `score_min ∈ (0, 1)`：cosine 相似度域，越界让阈值恒真/恒假
// - `max_text_bytes >= 1`：0 让所有文档提不出文本全落 uncategorized
fn sanitize_classify(classify: &mut ClassifyConfig) {
    let defaults = ClassifyConfig::default();
    face::sanitize_face_unit_open(
        &mut classify.score_min,
        defaults.score_min,
        "backend.classify.score_min",
    );
    if classify.max_text_bytes == 0 {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.classify.max_text_bytes",
            fallback = defaults.max_text_bytes,
            "max_text_bytes must be >= 1; falling back to default"
        );
        classify.max_text_bytes = defaults.max_text_bytes;
    }
}
