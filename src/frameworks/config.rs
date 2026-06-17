// 配置加载：从文件系统 / 环境变量读取并解析为 [`Config`]。
// Config 结构体定义在 usecases::config；本模块只负责 IO + 解析。
use std::env;
use std::fs;
use std::sync::OnceLock;

use tracing::debug;
use tracing::warn;

use crate::usecases::config::{
    Config, CopyConfig, FaceConfig, LogConfig, OcrConfig, validate_archive_template,
};

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
    sanitize_ocr(&mut cfg.backend.ocr);
    sanitize_face(&mut cfg.backend.face);
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
}

// 开区间 (0.0, 1.0) 内的有限正数。NaN/Inf 均通过 `is_finite()` 拒绝。
fn is_unit_open(v: f32) -> bool {
    v.is_finite() && v > 0.0 && v < 1.0
}

// FaceConfig 各阈值/权重越界即 warn + 回退默认，同 `sanitize_ocr` 哲学：
// 配置错误不让 cull 子命令静默全失败，但必须可观测。
// - `phash_hamming_max ∈ [1, 64]`：0 让所有图不分组、>64 让全图集成一大组
// - `sharpness_min > 0` 有限值：≤0 关粗筛、NaN/Inf 让 `<` 比较全 false 让所有图都过
// - 比例阈值（cosine/EAR/EyeState）∈ (0,1)：越界让判定恒真/恒假
// - 评分权重 `w_*` 必须有限非负：负值反转语义、NaN 让 score 全 NaN
fn sanitize_face(face: &mut FaceConfig) {
    const MAX_HAMMING: u8 = 64;

    let defaults = FaceConfig::default();
    if face.phash_hamming_max == 0 || face.phash_hamming_max > MAX_HAMMING {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.face.phash_hamming_max",
            value = face.phash_hamming_max,
            fallback = defaults.phash_hamming_max,
            "phash_hamming_max must be in [1, 64]; falling back to default"
        );
        face.phash_hamming_max = defaults.phash_hamming_max;
    }
    if !face.sharpness_min.is_finite() || face.sharpness_min <= 0.0 {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.face.sharpness_min",
            value = face.sharpness_min,
            fallback = defaults.sharpness_min,
            "sharpness_min must be a finite positive number; falling back to default"
        );
        face.sharpness_min = defaults.sharpness_min;
    }
    sanitize_face_unit_open(
        &mut face.face_cosine_min,
        defaults.face_cosine_min,
        "backend.face.face_cosine_min",
    );
    sanitize_face_unit_open(
        &mut face.ear_blink_max,
        defaults.ear_blink_max,
        "backend.face.ear_blink_max",
    );
    sanitize_face_unit_open(
        &mut face.eye_blink_score_max,
        defaults.eye_blink_score_max,
        "backend.face.eye_blink_score_max",
    );
    sanitize_face_weight(
        &mut face.w_sharpness,
        defaults.w_sharpness,
        "backend.face.w_sharpness",
    );
    sanitize_face_weight(&mut face.w_blink, defaults.w_blink, "backend.face.w_blink");
    sanitize_face_weight(&mut face.w_smile, defaults.w_smile, "backend.face.w_smile");
    sanitize_max_image_bytes(face, &defaults);
}

// max_image_bytes 太小会让所有图都被判超限跳过整个 cull pipeline；
// 1 MiB 以下没有业务场景（JPEG 缩略图都 > 100 KiB），统一收紧到 ≥ 1 MiB。
fn sanitize_max_image_bytes(face: &mut FaceConfig, defaults: &FaceConfig) {
    const MIN_IMAGE_BYTES: u64 = 1024 * 1024;
    if face.max_image_bytes < MIN_IMAGE_BYTES {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field = "backend.face.max_image_bytes",
            value = face.max_image_bytes,
            fallback = defaults.max_image_bytes,
            "max_image_bytes must be >= 1 MiB; falling back to default"
        );
        face.max_image_bytes = defaults.max_image_bytes;
    }
}

fn sanitize_face_unit_open(value: &mut f32, fallback: f32, field: &str) {
    if !is_unit_open(*value) {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field,
            value = *value,
            fallback,
            "value must be in (0, 1); falling back to default"
        );
        *value = fallback;
    }
}

fn sanitize_face_weight(value: &mut f32, fallback: f32, field: &str) {
    if !value.is_finite() || *value < 0.0 {
        warn!(
            feature = "config",
            operation = "sanitize",
            result = "invalid_value",
            field,
            value = *value,
            fallback,
            "weight must be a finite non-negative number; falling back to default"
        );
        *value = fallback;
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
        // 默认值可含嵌套占位符（`${A:-${B:-x}}`），name 未设时递归展开 default 才
        // 能让 B 等内层变量真正生效；否则字面 `${B:-x}` 会原样落进 YAML 值。
        env::var(name).unwrap_or_else(|_| expand_env(default))
    } else {
        // 无 `:-` 默认值的 bare `${VAR}` 在 env 未设时返空串：YAML 接受空字符串
        // 值，sanitize 只对 archive_template / log.level 等 fields 兜底，其他 string
        // 字段会静默吃下空串（如 backend.smb.default_user）。改返 warn 让运维可见
        // 配置漂移；行为仍兼容（保留旧空串语义）。
        env::var(body).unwrap_or_else(|_| {
            warn!(
                feature = "config",
                operation = "expand_env",
                result = "unset_var_empty_substitution",
                var = body,
                "bare ${{{body}}} unset; substituting empty string. Use ${{{body}:-default}} to silence."
            );
            String::new()
        })
    }
}

#[cfg(test)]
#[path = "config_test_common.rs"]
mod test_common;

#[cfg(test)]
#[path = "config_expand_tests.rs"]
mod expand_tests;

#[cfg(test)]
#[path = "config_load_tests.rs"]
mod load_tests;
