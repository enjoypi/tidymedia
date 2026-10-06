#![allow(unsafe_code)]

// SAFETY: nextest 默认每测试独立进程；测试期不存在并发读 env 的其他线程。
// 把 Rust 2024 unsafe std::env::{set_var, remove_var} 收敛到 set_env_var /
// remove_env_var 单一调用点，让 P0 §13 要求的 SAFETY 注释只需一份，
// 同时避免散在各测试体内的 unsafe 块拖低可读性（CLAUDE.md「DRY」）。
pub(super) fn set_env_var(name: &str, value: &str) {
    // SAFETY: see module-level invariant above
    unsafe { std::env::set_var(name, value) };
}

pub(super) fn remove_env_var(name: &str) {
    // SAFETY: see module-level invariant above
    unsafe { std::env::remove_var(name) };
}
