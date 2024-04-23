use serde_derive::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExifError {
    #[error("IO error occurred: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse an json: {0}")]
    ParseError(#[from] serde_json::Error),
}

const EXIFTOOL_ARGS: [&str; 14] = [
    "-a",
    "-charset",
    "filename=UTF8",
    "-d",
    "%s",
    "-ee",
    "--ext",
    "json",
    "-G",
    "-j",
    "-L",
    "-q",
    "-r",
    "-sort",
];

#[derive(Clone, Default, Deserialize)]
pub struct Exif {
    #[serde(rename = "SourceFile")]
    pub source_file: Option<String>,

    #[serde(rename = "File:FileModifyDate")]
    pub file_modify_date: Option<u64>,

    #[serde(rename = "File:FileCreateDate")]
    pub file_create_date: Option<u64>,

    #[serde(rename = "File:FileType")]
    pub file_type: Option<String>,

    #[serde(rename = "File:ImageHeight")]
    pub image_height: Option<u64>,

    #[serde(rename = "File:ImageWidth")]
    pub image_width: Option<u64>,

    #[serde(rename = "File:MIMEType")]
    pub mime_type: Option<String>,

    #[serde(rename = "EXIF:CreateDate")]
    pub exif_create_date: Option<u64>,

    #[serde(rename = "EXIF:ModifyDate")]
    pub exif_modify_date: Option<u64>,

    #[serde(rename = "EXIF:DateTimeOriginal")]
    pub date_time_original: Option<u64>,

    #[serde(rename = "EXIF:Model")]
    pub model: Option<String>,

    #[serde(rename = "H264:DateTimeOriginal")]
    pub h264_date_time_original: Option<u64>,

    #[serde(rename = "QuickTime:MediaCreateDate")]
    pub qt_date_time: Option<u64>,

    #[serde(rename = "XMP:PhotoId")]
    pub xmp_photo_id: Option<String>,

    #[serde(rename = "Composite:GPSLatitude")]
    pub gps_latitude: Option<String>,

    #[serde(rename = "Composite:GPSLongitude")]
    pub gps_longitude: Option<String>,

    #[serde(rename = "ExifTool:Error")]
    pub exif_tool_error: Option<String>,

    #[serde(rename = "ExifTool:Warning")]
    pub exif_tool_warning: Option<String>,
}

impl Exif {
    pub fn from(path: &str) -> Result<Option<Self>, ExifError> {
        let output = std::process::Command::new("exiftool")
            .args(&EXIFTOOL_ARGS)
            .arg(path)
            .output()?;

        let output = String::from_utf8(output.stdout).unwrap();
        let ret: [Self; 1] = serde_json::from_str(&output)?;
        Ok(Some(ret[0].clone()))
    }
}

#[cfg(test)]
mod test {
    use super::Exif;
    use super::super::test_common as common;

    #[test]
    fn test_exif() -> common::Result {
        let exif = Exif::from(common::DATA_SMALL).unwrap().unwrap();
        assert_eq!(
            exif.source_file.as_ref().unwrap().as_str(),
            common::DATA_SMALL
        );
        assert_eq!(exif.file_modify_date.unwrap(), 1692258850);
        Ok(())
    }
}
