//! 思维导图 zip 容器（xmind/itmz/mindnode/mmap）按 mime 子分流。commit 9 接入主体。

use crate::entities::backend::MediaReader;

pub(super) fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let _ = reader;
    let _ = mime;
    (0, 0)
}
