mod image;
mod mime;
mod types;
mod video;

pub use self::types::Exif;

// 测试要访问的内部 helper 在父 mod 私有 re-export，
// 让 `exif_tests.rs` 的 `super::xxx` 引用照常解析（CLAUDE.md「测试要访问的内部项」节）。
#[cfg(test)]
use self::image::build_gps_utc;
#[cfg(test)]
use self::image::parse_gps_date;
#[cfg(test)]
use self::image::populate_image_dates;
#[cfg(test)]
use self::image::populate_image_xmp_fallback;
#[cfg(test)]
use self::image::rational_to_u32;
#[cfg(test)]
use self::types::entry_value_to_epoch;
#[cfg(test)]
use self::video::ascii_datetime_to_epoch;
#[cfg(test)]
use self::video::populate_video_dates;
#[cfg(test)]
use self::mime::bmff_3gpp_mime;
#[cfg(test)]
use self::mime::m2ts_legacy_mime;
#[cfg(test)]
use self::mime::quicktime_legacy_mime;
#[cfg(test)]
use super::backend::MediaReader;

#[cfg(test)]
#[path = "exif_tests_common.rs"]
mod tests_common;

#[cfg(test)]
#[path = "exif_basic_tests.rs"]
mod basic_tests;

#[cfg(test)]
#[path = "exif_image_tests.rs"]
mod image_tests;

#[cfg(test)]
#[path = "exif_gps_tests.rs"]
mod gps_tests;

#[cfg(test)]
#[path = "exif_video_tests.rs"]
mod video_tests;

#[cfg(test)]
#[path = "exif_mime_tests.rs"]
mod mime_tests;

#[cfg(test)]
#[path = "exif_xmp_tests.rs"]
mod xmp_tests;
