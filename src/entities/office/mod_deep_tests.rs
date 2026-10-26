use std::io::Cursor;

use super::{extract_office_text, populate_office_dates};

#[test]
fn extract_office_text_routes_rtf_text_mime() {
    let mut reader = Cursor::new(br"{\rtf1 body}".to_vec());
    assert_eq!(extract_office_text(&mut reader, "text/rtf", 64), "body");
}

#[test]
fn populate_office_dates_routes_rtf_text_mime() {
    let rtf = br"{\rtf1\ansi{\info{\creatim\yr2017\mo2\dy14\hr10\min30\sec0}{\revtim\yr2018\mo1\dy1\hr12\min0\sec0}}}";
    let mut reader = Cursor::new(rtf.to_vec());
    let (c, m) = populate_office_dates(&mut reader, "text/rtf");
    assert_eq!(c, 1_487_068_200);
    assert_eq!(m, 1_514_808_000);
}
