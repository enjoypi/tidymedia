//! 外部 exiftool TSV 注入解析：把 skill `extract_exif.ts`（契约在
//! `.claude/skills/tidy-verify/config.yaml` 的 `exiftool_tsv_p`）的 8 列表转成
//! 可对账的 `ExifRow`，并按其 P0..P4 列序取「期望桶」。契约单点：本文件与
//! `config.yaml` 的 `-p` 列序必须互指同步。

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
#[path = "exif_tsv_tests.rs"]
mod tests;
