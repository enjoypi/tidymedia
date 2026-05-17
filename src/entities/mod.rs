// Entities 层：业务对象 + 业务规则。
// 注意：file_info/file_index/exif 当前混入了文件 IO 与 nom-exif/infer 库调用。
// 严格按 Clean Architecture 这些应进 Adapter 的 Gateway，但本仓库是 CLI
// 单体工具，无替换框架/DB 的实际场景；按 YAGNI 暂不再抽 Gateway 抽象。
use generic_array::typenum;
use generic_array::GenericArray;

pub type SecureHash = GenericArray<u8, typenum::U64>;

pub mod common;
pub(crate) mod exif;
pub mod file_index;
pub mod file_info;
pub mod media_time;
#[cfg(test)]
pub(crate) mod test_common;
