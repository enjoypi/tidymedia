// Gateway 实现：把 Backend trait 的各个存储后端适配器集中在此目录。
// trait 定义 + 值类型留在 entities::backend；本目录只放实现。
pub mod adb;
pub mod factory;
#[cfg(any(
    feature = "smb-backend",
    feature = "mtp-backend",
    feature = "adb-backend"
))]
mod factory_real;
#[cfg(test)]
pub(crate) mod fake_remote;
pub mod local;
pub mod mtp;
pub mod remote;
pub mod smb;

#[doc(hidden)]
pub mod fake;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
