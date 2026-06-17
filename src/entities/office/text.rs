//! 纯文本族（txt/md/rst/csv/tsv/log）：无 metadata，直接返 (0, 0)。
//! 让 `Info::create_time` 退到 P2 文件名 + P4 mtime。
//! 此模块不会有 commit 实现主体——纯文本设计即为「容器无时间」。

use crate::entities::backend::MediaReader;

pub(super) fn parse(reader: &mut dyn MediaReader, mime: &str) -> (u64, u64) {
    let _ = reader;
    let _ = mime;
    (0, 0)
}
