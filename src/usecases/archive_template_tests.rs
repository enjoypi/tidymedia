#[cfg(test)]
mod test_render {
    use crate::entities::exif::Exif;
    use crate::usecases::archive_template::{TemplateContext, render};

    fn ctx<'a>(
        year: &'a str,
        month: &'a str,
        day: &'a str,
        valuable_name: &'a str,
        exif: Option<&'a Exif>,
    ) -> TemplateContext<'a> {
        TemplateContext {
            year,
            month,
            day,
            valuable_name,
            exif,
        }
    }

    #[test]
    fn render_default_template_with_valuable_name() {
        let c = ctx("2024", "01", "15", "假日相册", None);
        let result = render("{year}/{month}/{valuable_name}", &c);
        assert_eq!(result, "2024/01/假日相册");
    }

    #[test]
    fn render_template_with_day() {
        let c = ctx("2024", "01", "15", "", None);
        let result = render("{year}/{month}/{day}", &c);
        assert_eq!(result, "2024/01/15");
    }

    #[test]
    fn render_template_without_valuable_name_drops_empty_segment() {
        // valuable_name 为空串时段被过滤
        let c = ctx("2024", "01", "15", "", None);
        let result = render("{year}/{month}/{valuable_name}", &c);
        assert_eq!(result, "2024/01");
    }

    #[test]
    fn render_template_with_model_from_exif() {
        let exif = Exif::with_mime("image/jpeg").with_make_model("Canon", "EOS R5");
        let c = ctx("2024", "01", "01", "", Some(&exif));
        let result = render("{year}/{model}", &c);
        assert_eq!(result, "2024/EOS R5");
    }

    #[test]
    fn render_template_with_make_from_exif() {
        let exif = Exif::with_mime("image/jpeg").with_make_model("Sony", "A7IV");
        let c = ctx("2024", "01", "01", "", Some(&exif));
        let result = render("{make}/{year}", &c);
        assert_eq!(result, "Sony/2024");
    }

    #[test]
    fn render_template_model_missing_uses_unknown() {
        // exif 存在但 model 为空 → "unknown"
        let exif = Exif::with_mime("image/jpeg");
        let c = ctx("2024", "01", "01", "", Some(&exif));
        let result = render("{model}/{year}", &c);
        assert_eq!(result, "unknown/2024");
    }

    #[test]
    fn render_template_make_missing_no_exif_uses_unknown() {
        let c = ctx("2024", "01", "01", "", None);
        let result = render("{make}/{year}", &c);
        assert_eq!(result, "unknown/2024");
    }

    #[test]
    fn render_template_model_missing_no_exif_uses_unknown() {
        let c = ctx("2024", "01", "01", "", None);
        let result = render("{model}/{year}", &c);
        assert_eq!(result, "unknown/2024");
    }

    #[test]
    fn render_all_placeholders() {
        let exif = Exif::with_mime("image/jpeg").with_make_model("Nikon", "Z9");
        let c = ctx("2025", "03", "07", "春节", Some(&exif));
        let result = render("{year}/{month}/{day}/{make}/{model}/{valuable_name}", &c);
        assert_eq!(result, "2025/03/07/Nikon/Z9/春节");
    }

    #[test]
    fn render_no_placeholders_returns_literal() {
        let c = ctx("2024", "01", "01", "", None);
        let result = render("archive/photos", &c);
        assert_eq!(result, "archive/photos");
    }

    /// EXIF Make 含 `../` 路径分隔符 → 经 `sanitize_path_segment` 替换为 `_`，
    /// 避免 `{make}` 模板被恶意 EXIF 注入致写入 output 父目录。
    #[test]
    fn render_template_sanitizes_make_with_path_separators() {
        let exif = Exif::with_mime("image/jpeg").with_make_model("../evil", "Z9");
        let c = ctx("2024", "01", "01", "", Some(&exif));
        let result = render("{make}/{year}", &c);
        assert_eq!(result, ".._evil/2024");
    }

    /// EXIF Model 含反斜杠（Windows 风格）→ 同样替换为 `_`。
    #[test]
    fn render_template_sanitizes_model_with_backslash() {
        let exif = Exif::with_mime("image/jpeg").with_make_model("Sony", r"A7\trap");
        let c = ctx("2024", "01", "01", "", Some(&exif));
        let result = render("{model}", &c);
        assert_eq!(result, "A7_trap");
    }

    /// EXIF Make 全 `.` 段（`.` / `..` / `...`）→ 整段被替换为同长度 `_`，
    /// 防止逐段 `Utf8PathBuf::join` 拼出 `output/..` 跳目录。
    #[test]
    fn render_template_sanitizes_dot_only_make() {
        let exif = Exif::with_mime("image/jpeg").with_make_model("..", "Z9");
        let c = ctx("2024", "01", "01", "", Some(&exif));
        let result = render("{make}", &c);
        assert_eq!(result, "__");
    }

    /// EXIF Make 含控制字符（\n 等）→ 替换为 `_`，避免日志/路径错位。
    #[test]
    fn render_template_sanitizes_control_chars_in_make() {
        let exif = Exif::with_mime("image/jpeg").with_make_model("a\nb\tc", "Z9");
        let c = ctx("2024", "01", "01", "", Some(&exif));
        let result = render("{make}", &c);
        assert_eq!(result, "a_b_c");
    }
}
