// Frameworks 层：配置加载、DI 组装、FFI 入口等框架基础设施。
pub mod config;

#[cfg(feature = "android-app")]
pub mod mobile;
