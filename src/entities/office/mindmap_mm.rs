//! FreeMind/FreePlane `.mm` XML 字节扫描 `<node CREATED="ms" MODIFIED="ms">`。commit 9 接入主体。

use crate::entities::backend::MediaReader;

pub(super) fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let _ = reader;
    let _ = mime;
    (0, 0)
}
