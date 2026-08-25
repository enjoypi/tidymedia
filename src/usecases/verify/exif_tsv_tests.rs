use super::{ExifRow, expected_bucket, normalize_sep, parse_tsv};

fn row(fields: [&str; 5]) -> ExifRow {
    ExifRow {
        path: String::new(),
        fields: fields.map(str::to_owned),
        make: String::new(),
        model: String::new(),
    }
}

#[test]
fn parse_tsv_skips_short_rows_and_normalizes_sep() {
    let rows = parse_tsv("a\tb\nC:\\x.jpg\t2020:07:01 10:00:00\t-\t-\t-\t-\t-\t-\n");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "C:/x.jpg");
}

#[test]
fn expected_bucket_prefers_dto_first() {
    let r = row([
        "2021:03:05 08:00:00",
        "2021:03:06 09:00:00+08:00",
        "-",
        "-",
        "-",
    ]);
    let (b, from) = expected_bucket(&r, 8);
    assert_eq!(b.as_deref(), Some("2021:03"));
    assert_eq!(from.as_deref(), Some("DTO"));
}

#[test]
fn expected_bucket_qt_column_converts_tz() {
    let r = row(["-", "2020:07:31 23:00:00", "-", "-", "-"]);
    let (b, from) = expected_bucket(&r, 8);
    assert_eq!(b.as_deref(), Some("2020:08"));
    assert_eq!(from.as_deref(), Some("QTCreationDate"));
}

#[test]
fn expected_bucket_none_when_all_fields_empty() {
    let r = row(["-", "-", "-", "-", "-"]);
    let (b, from) = expected_bucket(&r, 8);
    assert_eq!(b, None);
    assert_eq!(from, None);
}

#[test]
fn normalize_sep_replaces_backslashes() {
    assert_eq!(normalize_sep(r"a\b\c"), "a/b/c");
}
