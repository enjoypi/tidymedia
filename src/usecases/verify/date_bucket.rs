use super::re_year_month;
use super::re_yy_mm_dd;
use super::re_yyyymmdd;

pub(crate) fn parse_path_date_bucket(s: &str) -> Option<String> {
    if let Some(c) = re_yyyymmdd().captures(s) {
        return Some(format!(
            "{}:{}",
            c.get(1).expect("internal: group 1").as_str(),
            c.get(2).expect("internal: group 2").as_str()
        ));
    }
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
    if let Some(c) = re_yy_mm_dd().captures(s) {
        let yy: i32 = c
            .get(1)
            .expect("internal: group 1")
            .as_str()
            .parse()
            .expect("internal: two ASCII digits always fit i32");
        let year = if yy < 50 { 2000 + yy } else { 1900 + yy };
        return Some(format!(
            "{year:04}:{}",
            c.get(2).expect("internal: group 2").as_str()
        ));
    }
    None
}
