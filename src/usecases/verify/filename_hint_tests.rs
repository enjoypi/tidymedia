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
fn yyyymmdd_pure_compact() {
    assert_eq!(
        parse_path_date_bucket("20210611").as_deref(),
        Some("2021:06")
    );
}

#[test]
fn yyyymmdd_rejects_invalid_date_falls_through_to_year_month() {
    assert_eq!(
        parse_path_date_bucket("20211301120000.jpg").as_deref(),
        Some("2021:01")
    );
}

#[test]
fn delimited_full_date_keeps_year_month_precision() {
    assert_eq!(
        parse_path_date_bucket("2023-10-25.jpg").as_deref(),
        Some("2023:10")
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
fn yy_mm_dd_pre_2000_century() {
    assert_eq!(
        parse_path_date_bucket("99-12-31.jpg").as_deref(),
        Some("1999:12")
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
