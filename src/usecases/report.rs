// JSON 报告值对象 + 输出抽象。serde derive 在编译期派生，本身不引入运行期序列化器
// 依赖；具体 JSON 编码 + 原子写盘由 [`ReportSink`] 实现承担（adapters 层）。

use std::time::Instant;

use serde_derive::Serialize;

/// wall-clock 毫秒；`u128 → u64` overflow（~5 亿年不可能触发）走饱和 `u64::MAX`。
/// 4 个 usecase 入口共用：`let start = Instant::now();` ... `duration_ms: elapsed_ms(start)`。
/// `coverage(off)`：耗时随宿主时钟波动，无法稳定断言。
#[must_use]
pub fn elapsed_ms(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// 结构化日志 `feature` 维度值：copy / move / find / cull / `move_text_shot`
/// 单点定义。所有 use case + report sink 从此 `use`，避免任一处 const 漂移让
/// tracing 聚合按 feature 维度分裂。
pub const FEATURE_COPY: &str = "copy";
pub const FEATURE_MOVE: &str = "move";
pub const FEATURE_FIND: &str = "find";
pub const FEATURE_CULL: &str = "cull";
pub const FEATURE_MOVE_TEXT_SHOT: &str = "move_text_shot";
pub const FEATURE_VERIFY: &str = "verify";

/// Move 复用 copy 流程（remove=true 即 move）；日志 feature 按用户实际子命令
/// 呈现，避免 `move` 命令输出 feature="copy" 误导排障。
#[must_use]
pub fn feature_of(remove: bool) -> &'static str {
    if remove { FEATURE_MOVE } else { FEATURE_COPY }
}

/// `Report.errors` Vec 软上限：海量失败让 Vec + JSON pretty-print 累积到 40+ MB
/// 驻留（Android 2 GB RAM FFI 场景 OOM）。所有 use case 共用此单点。
pub const ERRORS_SOFT_CAP: usize = 1000;

/// 把 `err` 追加到 `errors`，若已达 [`ERRORS_SOFT_CAP`] 则丢弃并置 `truncated=true`。
/// `failed` 计数不受 cap 限制，让 caller 独立累加（详情列表被截断不影响总数可观测）。
pub fn push_error_capped(errors: &mut Vec<ReportError>, truncated: &mut bool, err: ReportError) {
    if errors.len() >= ERRORS_SOFT_CAP {
        *truncated = true;
    } else {
        errors.push(err);
    }
}

/// 把 `src` 全部 error 追加到 `dst`，同样受 cap 保护；`src_truncated=true` 也
/// 传染到 `dst_truncated`。用于 rayon `tree-reduce` 归并 delta.errors。
pub fn extend_errors_capped(
    dst: &mut Vec<ReportError>,
    dst_truncated: &mut bool,
    src: Vec<ReportError>,
    src_truncated: bool,
) {
    if src_truncated {
        *dst_truncated = true;
    }
    for e in src {
        push_error_capped(dst, dst_truncated, e);
    }
}

/// copy / move 操作报告。`scanned` = walker 触达的所有文件总数（含被识别为非媒体而
/// 跳过、空文件、读不到的）；`copied` / `ignored` / `failed` 反映 `do_copy` 决策计数。
#[expect(
    clippy::struct_excessive_bools,
    reason = "dry_run / remove / include_non_media / doc_only / errors_truncated 五态互相独立，无法收敛为单一 enum：dry_run 是 CLI flag、remove 区分 copy/move、include_non_media 是媒体过滤、doc_only 区分 copy-doc/move-doc、errors_truncated 是软 cap 命中指示"
)]
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
    /// `copy-doc`/`move-doc` 路径：仅文档族入档，媒体与未知格式 skip。
    pub doc_only: bool,
    pub errors: Vec<ReportError>,
    /// `errors` Vec 是否因 [`ERRORS_SOFT_CAP`] 截断；`true` = 存在未记入 `errors`
    /// 的失败项，用户应看 `failed` 总数与结构化日志。
    pub errors_truncated: bool,
    /// use case 入口到 finalize 的 wall-clock 耗时（毫秒）。供 AI 分析吞吐 =
    /// `bytes_read / duration_ms`。溢出（>= 2^64 ms ≈ 5 亿年）走 `u64::MAX`。
    pub duration_ms: u64,
}

/// find 操作报告。`scanned` = Index 中实际入索引的文件总数（不仅是重复组路径数）；
/// `bytes_read` = 流式哈希过程中累计读取的字节数；`groups` 保留每组完整字段（size + paths）
/// 不展平，让下游按 size 过滤或排序时不丢信息（`render_script` 的 `# SIZE N` 注释亦此口径）。
#[derive(Debug, Default, Serialize)]
pub struct FindReport {
    pub scanned: usize,
    pub groups: Vec<DuplicateGroupReport>,
    pub bytes_read: u64,
    /// use case 入口到构造 report 的 wall-clock 耗时（毫秒）。见 `CopyReport::duration_ms`。
    pub duration_ms: u64,
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
    Verify(&'a crate::usecases::verify::VerifyReport),
}

/// 报告输出端：序列化格式 + 持久化机制由实现者决定（JSON 写盘 / stdout / 推送…）。
/// Use Case 仅持有 trait 引用，不知道协议与 IO 细节。单方法 `write` 替代旧 `write_copy` /
/// `write_find` 双方法 boilerplate（同时保持对象安全）。
pub trait ReportSink: Send + Sync {
    fn write(&self, report: &Report<'_>);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(path: &str) -> ReportError {
        ReportError {
            path: path.to_owned(),
            message: "m".to_owned(),
        }
    }

    /// cap 未超时正常 push；truncated 不翻转。
    #[test]
    fn push_error_capped_under_cap() {
        let mut v = Vec::new();
        let mut t = false;
        push_error_capped(&mut v, &mut t, err("a"));
        assert_eq!(v.len(), 1);
        assert!(!t);
    }

    /// cap 达到时丢弃新 err 并置 truncated=true；Vec 长度保持 cap。
    #[test]
    fn push_error_capped_at_cap_sets_truncated() {
        let mut v = (0..ERRORS_SOFT_CAP)
            .map(|i| err(&format!("f{i}")))
            .collect();
        let mut t = false;
        push_error_capped(&mut v, &mut t, err("over"));
        assert_eq!(v.len(), ERRORS_SOFT_CAP);
        assert!(t);
    }

    /// `extend_errors_capped` 的 `src_truncated=true` arm：即便 dst 未满，
    /// src 声明自身已 truncate 也应传染让 `dst_truncated=true`（防 delta 侧
    /// 早已丢失记录但 merge 后误报"完整"）。
    #[test]
    fn extend_errors_capped_propagates_src_truncated() {
        let mut dst = Vec::new();
        let mut dst_t = false;
        extend_errors_capped(
            &mut dst,
            &mut dst_t,
            vec![err("a")],
            /* src_truncated = */ true,
        );
        assert_eq!(dst.len(), 1);
        assert!(dst_t, "src_truncated=true 必须传染到 dst_truncated");
    }

    /// `extend_errors_capped` 的 `src_truncated=false` 常规路径：合并 src 全量到 dst 且
    /// `dst_truncated` 保持 false。
    #[test]
    fn extend_errors_capped_keeps_false_when_neither_full() {
        let mut dst = vec![err("prev")];
        let mut dst_t = false;
        extend_errors_capped(&mut dst, &mut dst_t, vec![err("a"), err("b")], false);
        assert_eq!(dst.len(), 3);
        assert!(!dst_t);
    }

    /// `feature_of(true)` = MOVE，`feature_of(false)` = COPY（双 arm 覆盖）。
    #[test]
    fn feature_of_maps_remove_flag() {
        assert_eq!(feature_of(true), FEATURE_MOVE);
        assert_eq!(feature_of(false), FEATURE_COPY);
    }
}
