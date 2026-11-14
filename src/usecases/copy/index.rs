//! 扫源建索引与重叠保护：source ⊄ output 校验 → `Index` 装配（visit → 剔除 →
//! `parse_exif` → P3 富集 → 内容分类）。

use tracing::debug;

use super::ops::type_allowed;
use super::run::{CopyJob, Source, configured_chrono_offset};
use crate::entities::common;
use crate::entities::common::{canonical_prefix, under_prefix};
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;

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

pub(super) fn build_source_index(job: &CopyJob<'_>, mut source: Index) -> Index {
    let flags = job.flags;
    let skip_ext = source_skip_extensions(flags.include_non_media, flags.doc_only);
    for (loc, backend) in job.sources {
        source.visit_location_skipping(loc, backend, skip_ext);
    }
    if source.overflowed() {
        return source;
    }
    let excluded = source.remove_under_prefix(&job.output_prefix);
    if excluded > 0 {
        debug!(
            feature = job.feature,
            operation = "exclude_output_subtree",
            result = "ok",
            excluded,
            output = %job.output_prefix,
            "excluded already-archived files under output from source index"
        );
    }
    source.parse_exif(
        configured_chrono_offset(),
        flags.include_non_media,
        flags.doc_only,
    );
    if let Some(provider) = job.sidecar {
        source.enrich_candidates(provider, &|i: &Info| {
            type_allowed(i, flags.include_non_media, flags.doc_only)
        });
    }
    if let Some(provider) = job.classifier {
        source.classify_documents(provider);
    }
    source
}

pub(super) fn scan_output(job: &CopyJob<'_>, mut output: Index) -> Index {
    if !matches!(job.output_backend.exists(&job.output_loc), Ok(false)) {
        output.visit_location(&job.output_loc, &job.output_backend);
    }
    output
}

pub(crate) fn source_skip_extensions(include_non_media: bool, doc_only: bool) -> &'static [String] {
    if include_non_media || doc_only {
        return &[];
    }
    &crate::usecases::config::config().copy.skip_extensions
}
