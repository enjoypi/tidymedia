// JSON 报告值对象 + 输出抽象。serde derive 在编译期派生，本身不引入运行期序列化器
// 依赖；具体 JSON 编码 + 原子写盘由 [`ReportSink`] 实现承担（adapters 层）。

use serde_derive::Serialize;

/// copy / move 操作报告。`scanned` = walker 触达的所有文件总数（含被识别为非媒体而
/// 跳过、空文件、读不到的）；`copied` / `ignored` / `failed` 反映 `do_copy` 决策计数。
#[derive(Debug, Serialize)]
pub struct CopyReport {
    /// walker 触达的源端文件总数（含 `skipped_empty` / `skipped_unreadable` / `walker_errors`）。
    pub scanned: usize,
    pub copied: usize,
    pub ignored: usize,
    pub failed: usize,
    /// 0 字节文件被跳过的数量（统计自 `Index::stats`）。
    pub skipped_empty: u64,
    /// 因 IO/权限失败无法读取元数据的文件数量。
    pub skipped_unreadable: u64,
    /// walker 自身（非 UTF-8 路径、metadata 失败）报错的 entry 数量。
    pub walker_errors: u64,
    pub dry_run: bool,
    pub remove: bool,
    pub include_non_media: bool,
    pub errors: Vec<ReportError>,
}

/// find 操作报告。`scanned` = Index 中实际入索引的文件总数（不仅是重复组路径数）；
/// `bytes_read` = 流式哈希过程中累计读取的字节数；`groups` 保留每组完整字段（size + paths）
/// 不展平，让下游按 size 过滤或排序时不丢信息（`render_script` 的 `# SIZE N` 注释亦此口径）。
#[derive(Debug, Default, Serialize)]
pub struct FindReport {
    pub scanned: usize,
    pub groups: Vec<DuplicateGroupReport>,
    pub bytes_read: u64,
}

/// 单个重复组的报告项：组内文件 size（同组共享）+ 路径列表。
#[derive(Debug, Default, Serialize)]
pub struct DuplicateGroupReport {
    /// 组内每个文件的字节数（同组所有文件 size 相同；重复判定靠 size + hash）。
    pub size: u64,
    /// 组内文件路径列表（保留组边界，不做 CSV 展平）。
    pub paths: Vec<String>,
}

/// 报告中的单条错误记录。
#[derive(Debug, Serialize)]
pub struct ReportError {
    pub path: String,
    pub message: String,
}

/// 「写一份报告」的统一入参枚举。trait object 安全（无泛型方法），且新增 Report 变体
/// 无需触发实现者升级（除非显式 match）。`feature` 由 sink 自行从枚举派生。
pub enum Report<'a> {
    Copy(&'a CopyReport),
    Find(&'a FindReport),
    MoveTextShot(&'a crate::usecases::move_text_shot::MoveTextShotReport),
    Cull(&'a crate::usecases::cull::CullReport),
}

/// 报告输出端：序列化格式 + 持久化机制由实现者决定（JSON 写盘 / stdout / 推送…）。
/// Use Case 仅持有 trait 引用，不知道协议与 IO 细节。单方法 `write` 替代旧 `write_copy` /
/// `write_find` 双方法 boilerplate（同时保持对象安全）。
pub trait ReportSink: Send + Sync {
    fn write(&self, report: &Report<'_>);
}
