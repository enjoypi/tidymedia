//! `Index` 的富集步骤（`parse_exif` / `enrich_candidates` / `classify_documents`）：
//! 从 `file_index.rs` 拆出保持主文件在 512 行限内（文件组织规则）。
//! 三者同为「尽力而为」并行富集：失败静默跳过，从不返回错误。

use std::sync::Arc;

use chrono::FixedOffset;
use rayon::iter::{ParallelBridge, ParallelIterator};

use super::Index;
use crate::entities::backend::Backend;
use crate::entities::exif;
use crate::entities::media_time;
use crate::entities::threadpool::install_io;
use crate::entities::uri::Location;

/// P3 sidecar 等外部时间候选的发现函数（依赖倒置：协议解析在 adapters 层，
/// entities 只接收转换好的 [`media_time::Candidate`]）。
/// 普通 fn 指针即可——provider 无状态、`Send + Sync`、可直接进 rayon 并行。
pub type CandidateProvider = fn(&Location, &Arc<dyn Backend>) -> Vec<media_time::Candidate>;

/// 文档内容分类闭包（copy-doc/move-doc 路径）：与 [`CandidateProvider`] 同款
/// 依赖倒置，但分类器需要捕获已加载的模型状态，裸 fn 指针装不下——改用
/// `Box<dyn Fn>`。Entity 签名只出现 `std::ops::Fn`，不 `use usecases::classify`，
/// 仍满足 CA 内向依赖规则。第三参是 `parse_exif` 已 sniff 好的 MIME（复用避免
/// 二次 sniff IO）。返回 `None` = 分类失败/低置信，Info 不设 category，
/// 渲染时兜底 uncategorized。
pub type TextClassifyProvider =
    Box<dyn Fn(&Location, &Arc<dyn Backend>, &str) -> Option<String> + Send + Sync>;

impl Index {
    /// 并行对每个 indexed 文件用 nom-exif + infer 读取元数据；解析失败的文件被
    /// 静默跳过（"尽力而为"语义）。从不返回错误。
    /// `local_offset` 用于解释 EXIF 内无时区的 NaiveDateTime（相机本地时区）。
    /// `parse_non_media=false` 时非媒体 MIME 在 sniff 后短路为 mime-only `Exif`
    /// （跳过 office/zip 整文件解析）；调用方在过滤非媒体文件时传 false 省 IO。
    /// `doc_only=true` 时反向短路：仅文档族整文件解析，媒体/未知 mime-only
    /// （`copy-doc`/`move-doc` 路径）。
    ///
    /// F1：`&mut self` 保留供顶层调用感知阶段边界；内部走 `DashMap` 的 `iter_mut`
    /// 拿各 shard 写锁（rayon `par_bridge` 上转并行 iter，跨 shard 天然无竞争）。
    pub fn parse_exif(&mut self, local_offset: FixedOffset, parse_non_media: bool, doc_only: bool) {
        // 同 visit_location：Exif::open_filtered 内调 backend.open_read（远端是
        // 整文件同步下载）是 I/O-bound，包 I/O 池避免阻塞 CPU 池线程。
        install_io(|| {
            self.files.iter_mut().par_bridge().for_each(|mut kv| {
                let info = kv.value_mut();
                if let Ok(e) = exif::Exif::open_filtered(
                    info.location(),
                    &info.backend(),
                    local_offset,
                    parse_non_media,
                    doc_only,
                ) {
                    info.set_exif(e);
                }
            });
        });
    }

    /// 并行对每个 indexed 文件调用 provider 注入额外时间候选（P3 sidecar 等），
    /// 与 `parse_exif` 同为"尽力而为"富集步骤：无 sidecar 时 provider 返空即可。
    pub fn enrich_candidates(&mut self, provider: CandidateProvider) {
        // provider 通常调 backend.read_to_string 读 sidecar（远端 stat + read），
        // 同 visit_location 是 I/O-bound，包 I/O 池。
        install_io(|| {
            self.files.iter_mut().par_bridge().for_each(|mut kv| {
                let info = kv.value_mut();
                let candidates = provider(info.location(), &info.backend());
                if !candidates.is_empty() {
                    info.add_candidates(candidates);
                }
            });
        });
    }

    /// 并行对每个 indexed 文件调用分类闭包写回 `Info.category`
    /// （copy-doc/move-doc 的 `{category}` 归档段）。非文档族文件（媒体/未知）
    /// 直接跳过——它们随后在 `do_copy` 被过滤，分类推理纯属浪费；provider 返
    /// `None` 时不写（文件保持未分类，渲染时兜底 uncategorized）。
    /// MUST 在 `parse_exif` 之后调用（依赖已 sniff 的 MIME）。
    pub fn classify_documents(&mut self, provider: &TextClassifyProvider) {
        // provider 内 open_read + extract_office_text + embedding 推理；IO 与
        // CPU 混合，走 I/O 池与 parse_exif 同池避免嵌套池饥饿。
        install_io(|| {
            self.files.iter_mut().par_bridge().for_each(|mut kv| {
                let info = kv.value_mut();
                if !info.is_office() {
                    return;
                }
                let mime = info
                    .exif_ref()
                    .map(|e| e.mime_type().to_string())
                    .unwrap_or_default();
                if let Some(category) = provider(info.location(), &info.backend(), &mime) {
                    info.set_category(category);
                }
            });
        });
    }
}
