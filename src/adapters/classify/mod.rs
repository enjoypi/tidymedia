//! 文档内容分类 adapter：tract-onnx bge embedding 实现 + 测试 Fake。

pub mod fake;

pub(crate) mod tract_embed;
pub(crate) mod tract_embed_real;

pub use self::tract_embed::build_classifier;
