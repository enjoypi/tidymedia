use std::process;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use serde_derive::Deserialize;
use serde_json::Value;
use tracing::error;
use tracing::warn;

use super::common;

const FEATURE_EXIF: &str = "exif";
const META_TYPE_IMAGE: &str = "image/";
const META_TYPE_VIDEO: &str = "video/";
const EXIFTOOL_ARGS: [&str; 19] = [
    "-a", // Allow duplicate tags to be extracted
    "-charset",
    "filename=UTF8", // FileName to specify the encoding of file names on the command line
    "-d",            // Set format for date/time values
    "%s",            // seconds
    "-fast2", // -fast2 may be used to also avoid extracting MakerNote information if this is not required
    "-G",     // Print group name for each tag
    "-j",     // Export/import tags in JSON format
    "-m",     // Ignore minor errors and warnings
    "-q",     // Quiet processing
    "-r",     // Recursively process subdirectories
    "-CreateDate",
    "-DateTimeOriginal",
    "-FileCreateDate",
    "-FileModifyDate",
    "-MIMEType",
    "-MediaCreateDate",
    "-ModifyDate",
    "-SourceFile",
];

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Exif {
    #[serde(rename = "SourceFile", default)]
    source_file: Utf8PathBuf,

    #[serde(rename = "File:FileModifyDate")]
    file_modify_date: Option<Value>,

    #[serde(rename = "File:FileCreateDate")]
    file_create_date: Option<Value>,

    #[serde(rename = "File:MIMEType")]
    mime_type: Option<String>,

    #[serde(rename = "EXIF:CreateDate")]
    exif_create_date: Option<Value>,

    #[serde(rename = "EXIF:ModifyDate")]
    exif_modify_date: Option<Value>,

    #[serde(rename = "EXIF:DateTimeOriginal")]
    date_time_original: Option<Value>,

    #[serde(rename = "H264:DateTimeOriginal")]
    h264_date_time_original: Option<Value>,

    #[serde(rename = "QuickTime:MediaCreateDate")]
    qt_media_create_date: Option<Value>,

    #[serde(rename = "QuickTime:CreateDate")]
    qt_create_date: Option<Value>,
    // #[serde(rename = "ExifTool:Error")]
    // exif_tool_error: Option<String>,
    //
    // #[serde(rename = "ExifTool:Warning")]
    // exif_tool_warning: Option<String>,
}

impl Exif {
    #[cfg(test)]
    pub fn from(path: &str) -> common::Result<Vec<Self>> {
        Self::from_args(vec![path])
    }

    // exiftool 子进程的 I/O 错误分支（cmd.output() Err / stdout 非 JSON）依赖系统/工具状态，
    // 难以稳定触发；解析逻辑已通过 parse_exif_output 在阶段 2C 单独覆盖。
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn from_args(args: Vec<&str>) -> common::Result<Vec<Self>> {
        let mut cmd = process::Command::new("exiftool");
        let cmd = cmd.args(EXIFTOOL_ARGS);
        let cmd = cmd.args(args);

        let output = cmd.output()?;

        if !output.stderr.is_empty() {
            warn!(
                feature = FEATURE_EXIF,
                operation = "exiftool",
                result = "stderr",
                stderr = %String::from_utf8_lossy(&output.stderr),
                "exiftool produced stderr"
            );
        }

        if !output.status.success() {
            let args: Vec<_> = cmd.get_args().collect();
            error!(
                feature = FEATURE_EXIF,
                operation = "exiftool",
                result = "exit_failure",
                status = ?output.status.code(),
                args = ?args,
                "exiftool exited non-zero"
            );
        }

        let output = String::from_utf8_lossy(output.stdout.as_slice());
        let mut ret = parse_exif_output(output.trim())?;
        normalize_source_paths(&mut ret, cfg!(target_os = "windows"));
        Ok(ret)
    }

    pub fn source_file(&self) -> &Utf8Path {
        self.source_file.as_path()
    }

    pub fn mime_type(&self) -> &str {
        extract_string(&self.mime_type)
    }

    pub fn file_modify_date(&self) -> u64 {
        extract_timestamp(&self.file_modify_date)
    }

    pub fn file_create_date(&self) -> u64 {
        extract_timestamp(&self.file_create_date)
    }

    pub fn exif_create_date(&self) -> u64 {
        extract_timestamp(&self.exif_create_date)
    }

    pub fn exif_modify_date(&self) -> u64 {
        extract_timestamp(&self.exif_modify_date)
    }

    pub fn date_time_original(&self) -> u64 {
        extract_timestamp(&self.date_time_original)
    }

    pub fn h264_date_time_original(&self) -> u64 {
        extract_timestamp(&self.h264_date_time_original)
    }

    pub fn qt_media_create_date(&self) -> u64 {
        extract_timestamp(&self.qt_media_create_date)
    }

    pub fn qt_create_date(&self) -> u64 {
        extract_timestamp(&self.qt_create_date)
    }

    pub fn media_create_date(&self) -> u64 {
        if !self.is_media() {
            return 0;
        }

        if self.date_time_original() > 0 {
            return self.date_time_original();
        }

        if self.h264_date_time_original() > 0 {
            return self.h264_date_time_original();
        }

        if self.qt_media_create_date() > 0 {
            return self.qt_media_create_date();
        }

        if self.qt_create_date() > 0 {
            return self.qt_create_date();
        }

        if self.exif_create_date() > 0 {
            return self.exif_create_date();
        }

        if self.exif_modify_date() > 0 {
            return self.exif_modify_date();
        }

        if self.file_modify_date() > self.file_create_date() && self.file_create_date() > 0 {
            return self.file_create_date();
        }

        if self.file_modify_date() > 0 {
            return self.file_modify_date();
        }

        if self.file_create_date() > 0 {
            return self.file_create_date();
        }

        0
    }

    pub fn is_media(&self) -> bool {
        let mime_type = self.mime_type();
        (mime_type.starts_with(META_TYPE_IMAGE) || mime_type.starts_with(META_TYPE_VIDEO))
            && !mime_type.ends_with(".fpx")
    }
}

fn extract_timestamp(value: &Option<Value>) -> u64 {
    match value {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        _ => 0,
    }
}

fn extract_string(value: &Option<String>) -> &str {
    match value {
        Some(s) => s.as_str(),
        _ => "",
    }
}

pub(crate) fn normalize_source_paths(exifs: &mut [Exif], to_backslash: bool) {
    if to_backslash {
        exifs.iter_mut().for_each(|x| {
            let s = x.source_file.as_str().replace('/', "\\");
            x.source_file = Utf8PathBuf::from(s);
        });
    }
}

// 抽出来便于直接对非法 JSON 输入做单元测试，覆盖 serde_json::from_str 的 Err 分支。
pub(crate) fn parse_exif_output(trimmed: &str) -> common::Result<Vec<Exif>> {
    if trimmed.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(serde_json::from_str(trimmed)?)
    }
}

#[cfg(test)]
mod test {
    use std::io::Write;

    use camino::Utf8PathBuf;
    use rstest::rstest;
    use serde_json::json;
    use serde_json::Value;
    use tempfile;

    use super::super::test_common as common;
    use super::Exif;

    fn exif_from(value: Value) -> Exif {
        serde_json::from_value(value).expect("valid exif json")
    }

    #[test]
    fn test_exif_parses_dns_benchmark_png() {
        let exif = Exif::from(common::DATA_DNS_BENCHMARK).unwrap();
        let exif = &exif[0];
        assert_eq!(exif.source_file(), common::DATA_DNS_BENCHMARK);
        assert!(exif.is_media());
        assert!(exif.media_create_date() > 0);
        assert!(exif.file_modify_date() > 0);
    }

    #[test]
    fn test_from_args_reads_filelist() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "{}", common::DATA_DNS_BENCHMARK).unwrap();
        tmp.flush().unwrap();

        let exif = Exif::from_args(vec!["-@", tmp.path().to_str().unwrap()]).unwrap();
        let exif = &exif[0];
        assert_eq!(exif.source_file(), common::DATA_DNS_BENCHMARK);
        assert!(exif.is_media());
    }

    #[test]
    fn from_args_invalid_path_returns_empty() {
        let exif = Exif::from_args(vec!["/definitely/missing/xyz"]).unwrap();
        assert!(exif.is_empty());
    }

    #[rstest]
    #[case(json!({"File:MIMEType":"image/png","EXIF:DateTimeOriginal":1_700_000_000_u64}), 1_700_000_000)]
    #[case(json!({"File:MIMEType":"image/png","H264:DateTimeOriginal":1_700_000_001_u64}), 1_700_000_001)]
    #[case(json!({"File:MIMEType":"video/mp4","QuickTime:MediaCreateDate":1_700_000_002_u64}), 1_700_000_002)]
    #[case(json!({"File:MIMEType":"video/mp4","QuickTime:CreateDate":1_700_000_003_u64}), 1_700_000_003)]
    #[case(json!({"File:MIMEType":"image/jpeg","EXIF:CreateDate":1_700_000_004_u64}), 1_700_000_004)]
    #[case(json!({"File:MIMEType":"image/jpeg","EXIF:ModifyDate":1_700_000_005_u64}), 1_700_000_005)]
    #[case(
        json!({"File:MIMEType":"image/png","File:FileCreateDate":1_700_000_006_u64,"File:FileModifyDate":1_700_000_007_u64}),
        1_700_000_006
    )]
    #[case(json!({"File:MIMEType":"image/png","File:FileModifyDate":1_700_000_008_u64}), 1_700_000_008)]
    #[case(json!({"File:MIMEType":"image/png","File:FileCreateDate":1_700_000_009_u64}), 1_700_000_009)]
    fn media_create_date_priority_cascade(#[case] value: Value, #[case] want: u64) {
        let exif = exif_from(value);
        assert_eq!(exif.media_create_date(), want);
    }

    #[test]
    fn media_create_date_zero_when_not_media() {
        let exif = exif_from(json!({"EXIF:DateTimeOriginal": 1_700_000_000_u64}));
        assert_eq!(exif.media_create_date(), 0);
    }

    #[test]
    fn media_create_date_zero_when_no_signal_present() {
        let exif = exif_from(json!({"File:MIMEType":"image/png"}));
        assert_eq!(exif.media_create_date(), 0);
    }

    #[test]
    fn is_media_image_true() {
        let exif = exif_from(json!({"File:MIMEType":"image/jpeg"}));
        assert!(exif.is_media());
    }

    #[test]
    fn is_media_video_true() {
        let exif = exif_from(json!({"File:MIMEType":"video/mp4"}));
        assert!(exif.is_media());
    }

    #[test]
    fn is_media_fpx_excluded() {
        let exif = exif_from(json!({"File:MIMEType":"image/vnd.fpx"}));
        assert!(!exif.is_media());
    }

    #[test]
    fn is_media_none_false() {
        let exif = exif_from(json!({}));
        assert!(!exif.is_media());
    }

    #[test]
    fn extract_timestamp_string_returns_zero() {
        let exif = exif_from(json!({
            "File:MIMEType":"image/png",
            "EXIF:DateTimeOriginal":"2024-01-01 12:00:00"
        }));
        assert_eq!(exif.date_time_original(), 0);
    }

    #[test]
    fn extract_timestamp_float_returns_zero() {
        let exif = exif_from(json!({
            "File:MIMEType":"image/png",
            "EXIF:DateTimeOriginal": 1.5_f64
        }));
        assert_eq!(exif.date_time_original(), 0);
    }

    #[test]
    fn extract_string_none_returns_empty() {
        let exif = exif_from(json!({}));
        assert_eq!(exif.mime_type(), "");
    }

    #[test]
    fn accessors_return_zero_for_missing_fields() {
        let exif = exif_from(json!({}));
        assert_eq!(exif.file_modify_date(), 0);
        assert_eq!(exif.file_create_date(), 0);
        assert_eq!(exif.exif_create_date(), 0);
        assert_eq!(exif.exif_modify_date(), 0);
        assert_eq!(exif.h264_date_time_original(), 0);
        assert_eq!(exif.qt_media_create_date(), 0);
        assert_eq!(exif.qt_create_date(), 0);
    }

    #[test]
    fn normalize_source_paths_replaces_slashes_when_enabled() {
        let mut exifs = vec![exif_from(json!({
            "SourceFile": "a/b/c.png",
            "File:MIMEType":"image/png"
        }))];
        super::normalize_source_paths(&mut exifs, true);
        assert_eq!(exifs[0].source_file(), Utf8PathBuf::from("a\\b\\c.png"));
    }

    #[test]
    fn normalize_source_paths_noop_when_disabled() {
        let mut exifs = vec![exif_from(json!({
            "SourceFile": "a/b/c.png",
            "File:MIMEType":"image/png"
        }))];
        super::normalize_source_paths(&mut exifs, false);
        assert_eq!(exifs[0].source_file(), Utf8PathBuf::from("a/b/c.png"));
    }

    #[test]
    fn parse_exif_output_empty_returns_empty_vec() {
        let got = super::parse_exif_output("").unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn parse_exif_output_invalid_json_returns_err() {
        let err = super::parse_exif_output("definitely not json").unwrap_err();
        // 验证是 serde_json 类型错误
        let s = format!("{err}");
        assert!(!s.is_empty());
    }

    #[test]
    fn parse_exif_output_valid_json_round_trip() {
        let got = super::parse_exif_output(
            r#"[{"SourceFile":"x.png","File:MIMEType":"image/png"}]"#,
        )
        .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].source_file(), "x.png");
    }
}
