//! Apple iWork (pages/numbers/key) `Metadata/Properties.plist` 解析。commit 5 接入主体。

use crate::entities::backend::MediaReader;

pub(super) fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let _ = reader;
    let _ = mime;
    (0, 0)
}
