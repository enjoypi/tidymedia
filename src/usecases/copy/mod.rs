pub(super) mod classify;
pub(super) mod copy_loop;
pub(super) mod index;
pub(super) mod naming;
pub(super) mod ops;
pub(super) mod reporting;
pub(super) mod run;

pub(crate) use self::classify::{make_classify_provider, template_needs_category};
pub(crate) use self::index::source_skip_extensions;
pub(crate) use self::run::{Source, copy_with_sidecar, resolved_template};

#[cfg(test)]
use self::naming::{any_non_english, extract_valuable_name, generate_unique_name};
#[cfg(test)]
use self::reporting::summary_result;
#[cfg(test)]
use self::run::CopyOpts;
#[cfg(test)]
use self::tests_common::copy;
#[cfg(test)]
use self::tests_common::do_copy_with_default_cache as do_copy;
#[cfg(test)]
use crate::entities::common::canonical_prefix;
#[cfg(test)]
use crate::entities::file_info::Info;
#[cfg(test)]
use camino::Utf8Path;
#[cfg(test)]
use time::UtcOffset;

#[cfg(test)]
#[path = "copy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "copy_tests_common.rs"]
mod tests_common;

#[cfg(test)]
#[path = "copy_io_error_tests.rs"]
mod io_error_tests;

#[cfg(test)]
#[path = "copy_advanced_tests.rs"]
mod advanced_tests;

#[cfg(test)]
#[path = "copy_output_tests.rs"]
mod output_tests;

#[cfg(test)]
#[path = "copy_doc_only_tests.rs"]
mod doc_only_tests;

#[cfg(test)]
#[path = "copy_generate_tests.rs"]
mod generate_tests;

#[cfg(test)]
#[path = "copy_overlap_tests.rs"]
mod overlap_tests;

#[cfg(test)]
#[path = "copy_parallel_tests.rs"]
mod parallel_tests;

#[cfg(test)]
#[path = "copy_abort_tests.rs"]
mod abort_tests;

#[cfg(test)]
#[path = "copy_png_fixture_tests.rs"]
mod png_fixture;

#[cfg(test)]
#[path = "copy_safety_tests.rs"]
mod safety_tests;

#[cfg(test)]
#[path = "copy_type_filter_tests.rs"]
mod type_filter_tests;
