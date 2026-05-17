// docs/media-time-detection.md §三：来源等级 P0–P4。
// 等级越小越权威；同等级冲突时由 resolve 取较早值。

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
    P4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    // P0 — 容器内"拍摄时刻"
    ExifDateTimeOriginal,
    QuickTimeCreationDate,
    MkvDateUtc,
    // P1 — 容器内"数字化/写入"
    ExifCreateDate,
    QuickTimeCreateDate,
    // P2 — 文件名启发式
    FilenameCamera,
    FilenamePhone,
    FilenameScreenshot,
    FilenameUnixMillis,
    // P3 — 旁路 sidecar
    XmpSidecar,
    GoogleTakeoutJson,
    // P4 — 文件系统兜底
    FsMtime,
}

impl Source {
    pub fn priority(self) -> Priority {
        match self {
            Source::ExifDateTimeOriginal
            | Source::QuickTimeCreationDate
            | Source::MkvDateUtc => Priority::P0,
            Source::ExifCreateDate | Source::QuickTimeCreateDate => Priority::P1,
            Source::FilenameCamera
            | Source::FilenamePhone
            | Source::FilenameScreenshot
            | Source::FilenameUnixMillis => Priority::P2,
            Source::XmpSidecar | Source::GoogleTakeoutJson => Priority::P3,
            Source::FsMtime => Priority::P4,
        }
    }
}
