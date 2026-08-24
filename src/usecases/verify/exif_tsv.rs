//! 外部 exiftool TSV 注入解析：把 skill `02_extract_exif.sh` 的 8 列契约表转成
//! 可对账的 `ExifRow`，并按其 P0..P4 列序取「期望桶」。契约单点：本文件与
//! `.claude/scripts/tidy-verify/02_extract_exif.sh` 的 `-p` 顺序必须互指同步。

use super::bucket::qt_bucket;

/// exiftool `-p` 列序（0-based 对应 8 列契约的 col 2..6）：
/// path / DTO / `QT:CreationDate` / `QT:CreateDate` / `CreateDate` / `FileModifyDate` / Make / Model。
const FROM_LABEL: [&str; 5] = [
    "DTO",
    "QTCreationDate",
    "QTCreateDate",
    "CreateDate",
    "FsMtime",
];
/// `QuickTime` 系字段（col 2/3，fields 0-based idx 1/2）是 UTC 语义，需 +tz 转桶。
const QT_COL_INDICES: [usize; 2] = [1, 2];

pub(crate) struct ExifRow {
    /// exiftool 输出路径（`\` 或 `/` 分隔），已归一化为 `/` 便于跨平台匹配。
    pub(crate) path: String,
    /// 五个时间字段（DTO / `QT:CreationDate` / `QT:CreateDate` / `CreateDate` / `FileModifyDate`）。
    pub(crate) fields: [String; 5],
    pub(crate) make: String,
    pub(crate) model: String,
}

/// 解析 8 列 tab 表；不足 8 列的行跳过（exiftool 碎片行不参与对账）。
pub(crate) fn parse_tsv(content: &str) -> Vec<ExifRow> {
    content
        .lines()
        .filter_map(|line| {
            let row: Vec<&str> = line.split('\t').collect();
            if row.len() < 8 {
                return None;
            }
            let fields = std::array::from_fn(|i| row[i + 1].trim().to_owned());
            Some(ExifRow {
                path: normalize_sep(row[0]),
                fields,
                make: row[6].trim().to_owned(),
                model: row[7].trim().to_owned(),
            })
        })
        .collect()
}

/// 按 P0..P4 列序取第一个非空时间字段的期望桶（`Some`）+ 来源标签；QT 列走
/// UTC→tz 转换，其余列前 7 字符即 `YYYY:MM`。无任何时间字段时 `(None, None)`
/// （skill 的 from=NONE 口径）。
pub(crate) fn expected_bucket(row: &ExifRow, tz_hours: i8) -> (Option<String>, Option<String>) {
    for (i, v) in row.fields.iter().enumerate() {
        // exiftool 时间格式 `YYYY:MM:DD ...`：第 5 字符（idx 4）是 `:`；`-` 空字段不满足。
        if v.len() >= 7 && v.as_bytes().get(4) == Some(&b':') {
            let bucket = if QT_COL_INDICES.contains(&i) {
                qt_bucket(v, tz_hours)
            } else {
                v.chars().take(7).collect()
            };
            return (Some(bucket), Some(FROM_LABEL[i].to_owned()));
        }
    }
    (None, None)
}

pub(crate) fn normalize_sep(s: &str) -> String {
    s.replace('\\', "/")
}

#[cfg(test)]
mod tests {
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
}
