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

/// `render` 支持的全部占位符名。`validate_archive_template` 据此拒绝未知占位符
///（未知名渲染时不被替换，会产生形如 `{foo}` 的字面目录段）。
/// 新增占位符时 MUST 同步 `render` 内的替换分支与此列表。
pub(crate) const PLACEHOLDERS: [&str; 6] =
    ["year", "month", "day", "valuable_name", "make", "model"];

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
        let make = read_exif_field(ctx.exif, Exif::make, "make");
        result = result.replace("{make}", &make);
    }
    if result.contains("{model}") {
        let model = read_exif_field(ctx.exif, Exif::model, "model");
        result = result.replace("{model}", &model);
    }

    // 过滤空段（如 `{valuable_name}` 替换后产生的空组件），拼回斜杠分隔路径。
    result
        .split('/')
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

// {make} / {model} 共用的字段读取：无值（EXIF 缺失或字段空串）→ warn + "unknown"。
// fn 指针入参（`Exif::make` / `Exif::model`）消除两份同体函数。
fn read_exif_field(
    exif: Option<&Exif>,
    get: fn(&Exif) -> Option<&str>,
    placeholder: &'static str,
) -> String {
    let value = exif.and_then(|e| get(e).filter(|s| !s.is_empty()));
    if value.is_none() {
        warn!(
            feature = FEATURE_COPY,
            operation = "render_template",
            placeholder,
            "EXIF field not found; substituting \"unknown\""
        );
    }
    sanitize_path_segment(value.unwrap_or("unknown"))
}

/// 清洗 EXIF 字面值，防止 `{make}` / `{model}` 模板下 EXIF `Make="../evil"` 这类
/// 恶意值经字面 replace 渗入路径后跨目录写入：路径分隔符、控制字符、`NUL` 一律
/// 替换为 `_`；纯 `.` 段（包括 `.` / `..` / `...`）整段替换为 `_`，避免与
/// `naming::generate_unique_name` 的逐段 `Utf8PathBuf::join` 拼出 `output/..`。
/// `#[inline(never)]`：opt-level=0 时小函数仍可能被 LLVM 内联到 `read_exif_field`
/// 让 region instrumentation 丢失，强制独立 codegen 保证覆盖率工具能看到本函数行。
#[inline(never)]
fn sanitize_path_segment(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if !cleaned.is_empty() && cleaned.chars().all(|c| c == '.') {
        return "_".repeat(cleaned.chars().count());
    }
    cleaned
}

#[cfg(test)]
#[path = "archive_template_tests.rs"]
mod tests;
