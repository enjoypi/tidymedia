// 归档目录模板渲染：把 `{year}` / `{month}` / `{day}` / `{model}` / `{make}` /
// `{valuable_name}` 等占位符替换为实际值，生成目标子目录相对路径。
//
// 设计决策：
// - 不引入模板引擎（R1：保持零外部依赖），仅用 `str::replace` 顺序展开。
// - `{model}` / `{make}` 无值时用 `"unknown"` 兜底并发 warn，对齐任务规格。
// - `{valuable_name}` 为空串时保持原样（下游 join 会产生空路径段，由 caller 处理）。

use tracing::warn;

use crate::entities::exif::Exif;

const FEATURE_COPY: &str = "copy";

pub struct TemplateContext<'a> {
    pub year: &'a str,
    pub month: &'a str,
    pub day: &'a str,
    pub valuable_name: &'a str,
    /// EXIF 句柄；仅在模板含 `{make}` 或 `{model}` 时被访问。
    pub exif: Option<&'a Exif>,
}

/// 渲染归档模板，返回去掉末尾空段的相对路径字符串。
///
/// 空段（由连续 `/` 或尾斜杠产生）被过滤掉，调用方用 `Path::join` 接续文件名即可。
pub fn render(template: &str, ctx: &TemplateContext<'_>) -> String {
    let mut result = template.to_string();

    result = result.replace("{year}", ctx.year);
    result = result.replace("{month}", ctx.month);
    result = result.replace("{day}", ctx.day);
    result = result.replace("{valuable_name}", ctx.valuable_name);

    // {make} / {model}：仅在模板包含时才访问 EXIF，避免无谓解析。
    if result.contains("{make}") {
        let make = read_make(ctx.exif);
        result = result.replace("{make}", &make);
    }
    if result.contains("{model}") {
        let model = read_model(ctx.exif);
        result = result.replace("{model}", &model);
    }

    // 过滤空段（如 `{valuable_name}` 替换后产生的空组件），拼回斜杠分隔路径。
    result
        .split('/')
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn read_make(exif: Option<&Exif>) -> String {
    let value = exif.and_then(|e| e.make().filter(|s| !s.is_empty()));
    if value.is_none() {
        warn!(
            feature = FEATURE_COPY,
            operation = "render_template",
            placeholder = "make",
            "EXIF Make not found; substituting \"unknown\""
        );
    }
    value.unwrap_or("unknown").to_string()
}

fn read_model(exif: Option<&Exif>) -> String {
    let value = exif.and_then(|e| e.model().filter(|s| !s.is_empty()));
    if value.is_none() {
        warn!(
            feature = FEATURE_COPY,
            operation = "render_template",
            placeholder = "model",
            "EXIF Model not found; substituting \"unknown\""
        );
    }
    value.unwrap_or("unknown").to_string()
}

#[cfg(test)]
#[path = "archive_template_tests.rs"]
mod tests;
