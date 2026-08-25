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
    /// 括号内紧凑时戳 `IMG_6489(20210611-174530)(1).jpg` 的 `(yyyyMMdd-HHmmss)`
    /// （原 IMG_ 拍摄命名被清理/备份工具加括号时戳污染）。黑名单归属待真实样本
    /// 实证——默认无票防下载时戳错误推翻 P0，实证为原图拍摄时间后再移除。
    FilenameBracketedCompact,
    /// QQ 导出：`QQ图片<14-digit YYYYMMDDHHMMSS>`（下载时戳类，与 mmexport 同因无票）。
    FilenameQqExport,
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
            | Source::FilenameDashedDateTime
            | Source::FilenameBracketedCompact
            | Source::FilenameQqExport => Priority::P2,
            Source::XmpSidecar | Source::GoogleTakeoutJson => Priority::P3,
            Source::FsMtime => Priority::P4,
        }
    }

    /// 多数派仲裁的 filename 票资格。下载时戳类（13 位 unix 毫秒 / mmexport / QQ
    /// 导出 / 括号内时戳）与 mtime 天然同源——下载器落盘即把 mtime 写成下载时刻，
    /// "互证"恒真是假象，不构成推翻 P0 的证据。黑名单制：新增 P2 来源默认有票，
    /// 与 `is_filename_source` 由 priority 推导的"免双写"约定同向；新增下载时戳类
    /// variant 时必须加入黑名单。
    pub(crate) fn is_majority_filename_vote(self) -> bool {
        self.priority() == Priority::P2
            && !matches!(
                self,
                Source::FilenameUnixMillis
                    | Source::FilenameWeChatExport
                    | Source::FilenameBracketedCompact
                    | Source::FilenameQqExport
            )
    }
}
