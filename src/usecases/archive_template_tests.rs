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
}
