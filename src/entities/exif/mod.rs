mod document;
mod exif_io;
mod image;
mod image_jpeg;
mod image_png;
mod image_rw2;
mod mime;
mod types;
mod video;

pub use self::types::Exif;

#[cfg(test)]
use self::image::apply_tiff_ifd;
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
use self::image_png::populate_png_dates;
#[cfg(test)]
use self::image_rw2::populate_rw2_dates;
#[cfg(test)]
use self::mime::bmff_3gpp_mime;
#[cfg(test)]
use self::mime::bmff_xavc_mime;
#[cfg(test)]
use self::mime::is_office_mime;
#[cfg(test)]
use self::mime::m2ts_legacy_mime;
#[cfg(test)]
use self::mime::mime_from_ext;
#[cfg(test)]
use self::mime::quicktime_legacy_mime;
#[cfg(test)]
use self::types::entry_value_to_epoch;
#[cfg(test)]
use self::video::ascii_datetime_to_epoch;
#[cfg(test)]
use self::video::populate_video_dates;
#[cfg(test)]
use super::backend::MediaReader;

#[cfg(test)]
#[path = "exif_io_tests.rs"]
mod io_tests;

#[cfg(test)]
#[path = "exif_fixture_tests.rs"]
pub(crate) mod tests_common;

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

#[cfg(test)]
#[path = "exif_png_tests.rs"]
mod png_tests;

#[cfg(test)]
#[path = "exif_rw2_tests.rs"]
mod rw2_tests;
