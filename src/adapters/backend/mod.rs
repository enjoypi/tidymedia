// Gateway 实现：把 Backend trait 的各个存储后端适配器集中在此目录。
// trait 定义 + 值类型留在 entities::backend；本目录只放实现。
pub mod factory;
pub mod local;
pub mod remote;
pub(crate) mod fake_remote;
pub mod smb;
pub mod adb;
pub mod mtp;

#[doc(hidden)]
pub mod fake;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
