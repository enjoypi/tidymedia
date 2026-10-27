use std::sync::Arc;

use dashmap::DashSet;
use tracing::debug;

use super::super::naming::generate_unique_name;
use super::super::run::CopyOpts;
use crate::entities::backend::{Backend, is_partial_move, output_write_error};
use crate::entities::common;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use crate::usecases::report::feature_of;

#[inline(never)]
pub(crate) fn do_copy(
    src: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &Index,
    mkdir_cache: &DashSet<Location>,
    index_authoritative: bool,
    opts: &CopyOpts<'_>,
) -> common::Result<bool> {
    let src_loc = src.location().clone();
    let src_display = src.full_path.as_str();
    let feature = feature_of(opts.remove);

    if !super::passes_type_filter(src, opts, feature, src_display) {
        return Ok(false);
    }

    if let Some(dup) = output_index.exists(src, true)? {
        debug!(
            feature,
            operation = "detect_duplicate",
            result = "duplicate",
            source = %src_display,
            duplicate = %dup,
            "source duplicates an existing file in output"
        );
        if opts.remove && !opts.dry_run {
            super::refuse_self_duplicate(&src_loc, &dup)?;
            src.backend().remove_file(&src_loc)?;
        }
        return Ok(false);
    }

    while let Some((target_dir_loc, target_loc)) = generate_unique_name(
        src,
        output_dir,
        output_backend,
        opts.template,
        output_index,
        index_authoritative,
    )? {
        if opts.dry_run {
            return record_dry_run(src, target_loc, output_backend, output_index, feature);
        }

        if !mkdir_cache.contains(&target_dir_loc) {
            output_backend
                .mkdir_p(&target_dir_loc)
                .map_err(output_write_error)?;
            mkdir_cache.insert(target_dir_loc);
        }

        if opts.remove
            && src
                .backend()
                .supports_native_rename_to(output_backend.as_ref())
        {
            match src.backend().rename(&src_loc, &target_loc, false) {
                Ok(()) => {
                    let target_display = target_loc.display();
                    debug!(
                        feature,
                        operation = "copy_file",
                        result = "ok",
                        source = %src_display,
                        target = %target_display,
                        "file transferred"
                    );
                    output_index.add(src.cloned_at(target_loc, Arc::clone(output_backend)));
                    return Ok(true);
                }
                Err(e) => {
                    if is_partial_move(&e) {
                        output_index
                            .add(src.cloned_at(target_loc.clone(), Arc::clone(output_backend)));
                        return Err(e.into());
                    }
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }

        match super::stream_copy(src, &target_loc, output_backend.as_ref()) {
            Err(common::Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            r => r?,
        }
        output_index.add(src.cloned_at(target_loc.clone(), Arc::clone(output_backend)));
        if opts.remove {
            super::remove_src_after_stream_copy(src, &src_loc, src_display, &target_loc)?;
        }
        let target_display = target_loc.display();
        debug!(
            feature,
            operation = "copy_file",
            result = "ok",
            source = %src_display,
            target = %target_display,
            "file transferred"
        );
        return Ok(true);
    }
    Err(common::Error::Io(std::io::Error::other(format!(
        "无法为\"{src_display}\"生成目标目录的文件名"
    ))))
}

fn record_dry_run(
    src: &Info,
    target_loc: Location,
    output_backend: &Arc<dyn Backend>,
    output_index: &Index,
    feature: &'static str,
) -> common::Result<bool> {
    let target_display = target_loc.display();
    debug!(
        feature,
        operation = "copy_file",
        result = "dry_run",
        source = %src.full_path,
        target = %target_display,
        "would transfer file"
    );
    src.secure_hash()?;
    output_index.add(src.cloned_at(target_loc, Arc::clone(output_backend)));
    Ok(true)
}
