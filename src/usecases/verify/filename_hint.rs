//! 文件名 / 路径日期桶解析（三正则，仅诊断提示，不进 `MediaTimeDecision`）。
//! 踩坑口径与 tidy-verify skill 逐条对齐：单数字月兼容（`西宁 2008-6-19`）、`YYYY`
//! 前非数字边界（防 `P1120296.JPG` 假阳）、alternation 长 token 优先
//! （`2008-10` 不被吃成 `2008-1`）。

use std::sync::OnceLock;

use regex::Regex;

fn re_year_month() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:^|[^0-9])(199[5-9]|20[0-2][0-9]|2030)[-_./ ]?(1[012]|0?[1-9])")
            .expect("internal: static regex")
    })
}

fn re_yyyymmdd() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:^|[^0-9])(199[5-9]|20[0-2][0-9]|2030)(0[1-9]|1[012])(0[1-9]|[12][0-9]|3[01])",
        )
        .expect("internal: static regex")
    })
}

fn re_yy_mm_dd() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:^|[^0-9])([0-9][0-9])-(0[1-9]|1[012])-(0[1-9]|[12][0-9]|3[01])(?:[^0-9]|$)")
            .expect("internal: static regex")
    })
}

/// 从相对路径（源根剥离后）中抽显式时间，返回首个 `YYYY:MM` 桶或 `None`。
/// 三正则按优先级尝试：`YYYY[-_./ ]?M` → `YYYYMMDD` → `YY-MM-DD`（`YY<50→20YY`）。
pub(crate) fn parse_path_date_bucket(s: &str) -> Option<String> {
    if let Some(c) = re_year_month().captures(s) {
        let mo = c.get(2).expect("internal: group 2").as_str();
        let mo = if mo.len() == 1 {
            format!("0{mo}")
        } else {
            mo.to_owned()
        };
        return Some(format!(
            "{}:{mo}",
            c.get(1).expect("internal: group 1").as_str()
        ));
    }
    if let Some(c) = re_yyyymmdd().captures(s) {
        return Some(format!(
            "{}:{}",
            c.get(1).expect("internal: group 1").as_str(),
            c.get(2).expect("internal: group 2").as_str()
        ));
    }
    if let Some(c) = re_yy_mm_dd().captures(s) {
        let yy: i32 = c.get(1).expect("internal: group 1").as_str().parse().ok()?;
        let year = if yy < 50 { 2000 + yy } else { 1900 + yy };
        return Some(format!(
            "{year:04}:{}",
            c.get(2).expect("internal: group 2").as_str()
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::parse_path_date_bucket;

    #[test]
    fn year_month_with_single_digit_month() {
        assert_eq!(
            parse_path_date_bucket("西宁 2008-6-19 13-08-21.jpg").as_deref(),
            Some("2008:06")
        );
    }

    #[test]
    fn year_month_long_token_wins_over_short() {
        assert_eq!(
            parse_path_date_bucket("2008-10-15.jpg").as_deref(),
            Some("2008:10")
        );
    }

    #[test]
    fn yyyymmdd_compact() {
        assert_eq!(
            parse_path_date_bucket("IMG_20210611_174530.jpg").as_deref(),
            Some("2021:06")
        );
    }

    #[test]
    fn yy_mm_dd_century_heuristic() {
        assert_eq!(
            parse_path_date_bucket("scan 13-08-21.jpg").as_deref(),
            Some("2013:08")
        );
    }

    #[test]
    fn prefix_non_digit_guard_rejects_embedded_year() {
        assert_eq!(parse_path_date_bucket("P1120296.JPG"), None);
    }

    #[test]
    fn no_date_returns_none() {
        assert_eq!(parse_path_date_bucket("IMG_0001.jpg"), None);
    }
}
