use chrono::FixedOffset;

use super::Exif;

pub(super) fn utc() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

pub(super) fn mk_exif(mime: &str, init: impl FnOnce(&mut Exif)) -> Exif {
    let mut exif = Exif {
        mime_type: mime.to_string(),
        ..Default::default()
    };
    init(&mut exif);
    exif
}
