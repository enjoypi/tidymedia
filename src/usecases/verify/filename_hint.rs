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

#[path = "filename_hint_phantom.rs"]
mod phantom;
#[doc(hidden)]
pub(crate) use self::phantom::parse_path_date_bucket;

#[cfg(test)]
#[path = "filename_hint_tests.rs"]
mod tests;
