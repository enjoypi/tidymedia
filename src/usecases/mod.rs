// Use Cases 层：编排 Entity 业务规则 + 应用级流程。
pub(super) use copy::Source;
pub(super) use copy::copy_with_sidecar;
pub(super) use find::find_duplicates;
#[cfg(feature = "ocr-detect")]
pub(super) use move_text_shot::move_text_shot;

pub(crate) mod config;

mod archive_template;
mod copy;
pub(crate) mod find;
#[cfg(feature = "ocr-detect")]
pub(crate) mod move_text_shot;
pub(crate) mod report;
