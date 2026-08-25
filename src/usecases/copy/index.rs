//! 扫源建索引与重叠保护：source ⊄ output 校验 → `Index` 装配（visit → 剔除 →
//! `parse_exif` → P3 富集 → 内容分类）。

use tracing::debug;

use super::run::{Source, configured_chrono_offset};
use crate::entities::common;
use crate::entities::common::{canonical_prefix, under_prefix};
use crate::entities::file_index::{CandidateProvider, Index, TextClassifyProvider};

// 重叠保护：source ⊆ output（canonical 前缀含相等）时，dedup 会把每个源文件判为
// output 中已存在的副本，move 模式下 remove 即删除文件自身——必须 fail fast。
pub(super) fn ensure_sources_outside_output(
    sources: &[Source],
    output_prefix: &str,
) -> common::Result<()> {
    for (loc, _) in sources {
        let src_prefix = canonical_prefix(loc);
        if under_prefix(&src_prefix, output_prefix) {
            return Err(common::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "source {src_prefix} is inside output {output_prefix}; \
                     move would treat sources as duplicates of themselves"
                ),
            )));
        }
    }
    Ok(())
}

// 扫源建索引 + 重叠剔除 + EXIF/P3 富集 + 内容分类；拆出让 copy_with_sidecar 保持
// 在 100 行内。include_non_media=false 时 parse_exif 对非媒体 MIME 短路（sniff 后
// 跳过整文件容器解析）——这些文件后续在 do_copy 被 is_media 过滤，解析成本纯属浪费。
// doc_only=true 反向短路：仅文档族整文件解析（copy-doc/move-doc 路径）。
// classifier 已由调用方按「doc_only && 模板含 {category}」过滤——无消费点不做工。
pub(super) fn build_source_index(
    sources: &[Source],
    output_prefixes: (&str, &str),
    sidecar: Option<CandidateProvider>,
    classifier: Option<TextClassifyProvider>,
    feature: &'static str,
    include_non_media: bool,
    doc_only: bool,
) -> Index {
    let (output_prefix, output_literal) = output_prefixes;
    let mut source = Index::new();
    for (loc, backend) in sources {
        source.visit_location(loc, backend);
    }
    // output ⊂ source（就地归档，如 copy /photos -o /photos/archive）：把已归档
    // 文件从 source 索引剔除，否则它们会被再次复制 / 在 move 模式下被误删。
    // canonical + 字面双前缀各剔一遍：index key 是 walker 字面路径，macOS
    // `/var → /private/var` 等 symlink 下 canonical 前缀匹配不到字面 key；
    // 两者相同时第二遍空剔除，幂等零副作用（免去相等判分支）。
    let excluded =
        source.remove_under_prefix(output_prefix) + source.remove_under_prefix(output_literal);
    if excluded > 0 {
        debug!(
            feature,
            operation = "exclude_output_subtree",
            result = "ok",
            excluded,
            output = %output_prefix,
            "excluded already-archived files under output from source index"
        );
    }
    source.parse_exif(configured_chrono_offset(), include_non_media, doc_only);
    // P3 富集：adapters 层注入的 sidecar 发现（XMP / Takeout），entities 只消费
    // 转换好的 Candidate（依赖倒置，协议细节不进 usecases）。
    if let Some(provider) = sidecar {
        source.enrich_candidates(provider);
    }
    // 内容分类（copy-doc/move-doc）：MUST 在 parse_exif 之后（复用已 sniff MIME）。
    if let Some(provider) = classifier {
        source.classify_documents(&provider);
    }
    source
}
