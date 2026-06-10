// Entities 层：业务对象 + 业务规则。
// 注意：file_info/file_index/exif 当前混入了文件 IO 与 nom-exif/infer 库调用。
// 严格按 Clean Architecture 这些应进 Adapter 的 Gateway，但本仓库是 CLI
// 单体工具，无替换框架/DB 的实际场景；按 YAGNI 暂不再抽 Gateway 抽象。

// sha2 0.11 起 Digest::Output 走 hybrid_array::Array；直接复用其类型而非
// 自己用 generic_array::GenericArray 重新声明，避免两套 array crate 不兼容。
pub type SecureHash = sha2::digest::Output<sha2::Sha512>;

pub mod backend;
pub mod common;
pub(crate) mod exif;
pub mod file_index;
pub mod file_info;
pub(crate) mod m2ts;
pub mod media_time;
pub(crate) mod riff;
#[cfg(test)]
pub(crate) mod test_common;
pub mod uri;
pub(crate) mod xmp;
