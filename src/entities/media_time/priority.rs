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
    /// 办公文档容器内创建时间（dcterms:created / PDF `/CreationDate` / CFB
    /// `PID_CREATE_DTM` / iWork plist `createdDate` / `.mm` CREATED 等），由
    /// `entities::office` 子模块归一为 Unix UTC epoch。
    DocumentCreated,
    // P1 — 容器内"数字化/写入"
    ExifCreateDate,
    QuickTimeCreateDate,
    // P2 — 文件名启发式
    FilenameCamera,
    FilenamePhone,
    FilenameVideoPhone,
    FilenameScreenshot,
    FilenameUnixMillis,
    FilenamePixel,
    FilenameBareYyyymmdd,
    FilenameWeChatExport,
    FilenameWhatsApp,
    /// 通用 `<任意前缀>YYYY-MM-DD HH-MM-SS`（事后批量重命名工具的常见格式）
    FilenameDashedDateTime,
    // P3 — 旁路 sidecar
    XmpSidecar,
    GoogleTakeoutJson,
    // P4 — 文件系统兜底
    FsMtime,
}

impl Source {
    #[must_use]
    pub fn priority(self) -> Priority {
        match self {
            Source::ExifDateTimeOriginal
            | Source::QuickTimeCreationDate
            | Source::MkvDateUtc
            | Source::DocumentCreated => Priority::P0,
            Source::ExifCreateDate | Source::QuickTimeCreateDate => Priority::P1,
            Source::FilenameCamera
            | Source::FilenamePhone
            | Source::FilenameVideoPhone
            | Source::FilenameScreenshot
            | Source::FilenameUnixMillis
            | Source::FilenamePixel
            | Source::FilenameBareYyyymmdd
            | Source::FilenameWeChatExport
            | Source::FilenameWhatsApp
            | Source::FilenameDashedDateTime => Priority::P2,
            Source::XmpSidecar | Source::GoogleTakeoutJson => Priority::P3,
            Source::FsMtime => Priority::P4,
        }
    }
}
