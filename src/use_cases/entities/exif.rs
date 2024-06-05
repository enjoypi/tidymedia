use std::process;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use serde_derive::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tracing::{error, warn};

#[derive(Debug, Error)]
pub enum ExifError {
    #[error("converting from utf8 error occurred: {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),

    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse an json: {0}")]
    Parse(#[from] serde_json::Error),
}

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
    #[serde(rename = "SourceFile")]
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
    pub fn from(path: &str) -> Result<Vec<Self>, ExifError> {
        Self::from_args(vec![path])
    }

    pub fn from_args(args: Vec<&str>) -> Result<Vec<Self>, ExifError> {
        let mut cmd = process::Command::new("exiftool");
        let cmd = cmd.args(EXIFTOOL_ARGS);
        let cmd = cmd.args(args);

        let output = cmd.output()?;

        if !output.stderr.is_empty() {
            warn!("{}", String::from_utf8_lossy(&output.stderr));
        }

        if !output.status.success() {
            let args: Vec<_> = cmd.get_args().collect();
            error!("exiftool failed {:?}", args);
        }

        let output = String::from_utf8(output.stdout)?;
        let mut ret: Vec<Exif> = serde_json::from_str(&output)?;
        #[cfg(target_os = "windows")]
        {
            ret.iter_mut().for_each(|x| {
                let s = x.source_file.as_str().replace('/', "\\");
                x.source_file = Utf8PathBuf::from(s);
            })
        }

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

    pub fn time_from_filename(&self) -> u64 {
        0
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

        if self.time_from_filename() > 0 {
            return self.time_from_filename();
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
        mime_type.starts_with(META_TYPE_IMAGE) || mime_type.starts_with(META_TYPE_VIDEO)
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

#[cfg(test)]
mod test {
    use std::io::Write;

    use tempfile;

    use super::super::test_common as common;
    use super::Exif;

    #[test]
    fn test_exif() -> common::Result {
        let exif = Exif::from(common::DATA_DNS_BENCHMARK)?;
        let exif = &exif[0];
        assert_eq!(exif.source_file(), common::DATA_DNS_BENCHMARK);
        assert_eq!(exif.file_modify_date(), 1706076164);
        assert_eq!(exif.media_create_date(), 1706076164);
        assert!(exif.is_media());
        Ok(())
    }

    #[test]
    fn test_from_args() -> common::Result {
        let mut tmp = tempfile::NamedTempFile::new()?;
        writeln!(tmp, "{}", common::DATA_DNS_BENCHMARK)?;
        tmp.flush()?;

        let exif = Exif::from_args(vec!["-@", tmp.path().to_str().unwrap()])?;
        let exif = &exif[0];
        assert_eq!(exif.source_file(), common::DATA_DNS_BENCHMARK);
        assert_eq!(exif.file_modify_date(), 1706076164);
        assert_eq!(exif.media_create_date(), 1706076164);
        assert!(exif.is_media());
        Ok(())
    }
}
