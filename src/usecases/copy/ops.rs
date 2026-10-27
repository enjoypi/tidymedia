use tracing::debug;

use super::run::CopyOpts;
use crate::entities::backend::{
    Backend, output_write_error, partial_move_error, stream_copy as backend_stream_copy,
};
use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use camino::Utf8Path;

#[path = "do_copy.rs"]
mod do_copy;
#[doc(hidden)]
pub(crate) use self::do_copy::do_copy;

pub(super) fn type_allowed(src: &Info, include_non_media: bool, doc_only: bool) -> bool {
    if doc_only {
        return src.is_office();
    }
    if include_non_media {
        return true;
    }
    src.is_media()
}

pub(super) fn count_type_filtered(source: &Index, opts: &CopyOpts<'_>) -> usize {
    source
        .iter()
        .filter(|kv| !type_allowed(kv.value(), opts.include_non_media, opts.doc_only))
        .count()
}

fn passes_type_filter(src: &Info, opts: &CopyOpts<'_>, feature: &'static str, disp: &str) -> bool {
    if type_allowed(src, opts.include_non_media, opts.doc_only) {
        return true;
    }
    debug!(
        feature,
        operation = "filter_media",
        result = "skipped_by_type",
        source = %disp,
        "file skipped by type filter"
    );
    false
}

fn remove_src_after_stream_copy(
    src: &Info,
    src_loc: &Location,
    src_display: &str,
    target_loc: &Location,
) -> common::Result<()> {
    src.backend()
        .remove_file(src_loc)
        .map_err(|re| {
            partial_move_error(
                re.kind(),
                format!(
                    "copy: copied {src_display} -> {dst} but cannot remove source: {re}",
                    dst = target_loc.display(),
                ),
            )
        })
        .map_err(common::Error::from)
}

#[inline(never)]
pub(super) fn refuse_self_duplicate(src: &Location, dup: &Utf8Path) -> common::Result<()> {
    let same = match src {
        Location::Local(p) => common::canonical_paths_equal(p, dup),
        Location::Smb { .. } => {
            common::canonical_prefix(src) == common::key_from_display(dup.as_str())
        }
    };
    if same {
        return Err(common::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "refusing to remove {}: it is the output duplicate {dup} itself",
                src.display()
            ),
        )));
    }
    Ok(())
}

fn stream_copy(src: &Info, target: &Location, out_be: &dyn Backend) -> common::Result<()> {
    let src_be = src.backend();
    backend_stream_copy(src_be.as_ref(), src.location(), out_be, target, false)
        .map(drop)
        .map_err(output_write_error)?;
    Ok(())
}
