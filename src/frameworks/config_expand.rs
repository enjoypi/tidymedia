// 配置环境变量展开：`${VAR:-default}` → env value 或默认值，带递归深度上限与
// yaml 注入防护。拆自 `config.rs`。
use std::env;

use tracing::warn;

/// 最大嵌套展开深度：防 `${A:-${A:-${A:-...}}}` 递归栈溢出。
/// 32 层覆盖任何合理嵌套（实际生产 ≤ 2 层），超此值返字面量 + warn。
const EXPAND_ENV_MAX_DEPTH: u8 = 32;

/// 把 `${VAR:-default}` 替换为环境变量值或默认值。
//
// `$` `{` `}` 都是 ASCII，UTF-8 多字节字符的字节绝不会撞上 ASCII 范围；
// 因此按字节扫描 placeholder 边界，剩余段以 `&input[..]` 切片整段 push，
// 保留原 UTF-8 编码不被逐字节降级为 Latin-1。
pub fn expand_env(input: &str) -> String {
    expand_env_depth(input, 0)
}

fn expand_env_depth(input: &str, depth: u8) -> String {
    if depth >= EXPAND_ENV_MAX_DEPTH {
        warn!(
            feature = "config",
            operation = "expand_env",
            result = "max_depth_reached",
            depth,
            "expand_env nesting exceeded limit; emitting literal to break recursion"
        );
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'$'
            && bytes[i + 1] == b'{'
            && let Some(end) = find_close_brace(bytes, i + 2)
        {
            out.push_str(&input[last..i]);
            out.push_str(&resolve_var(&input[i + 2..end], depth + 1));
            i = end + 1;
            last = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&input[last..]);
    out
}

// 按括号配对计数找闭合 `}`：默认值可含嵌套占位符
// （如 `${TMPL:-{year}/{month}}`），取第一个 `}` 会截断默认值产生非法 YAML。
fn find_close_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (off, &b) in bytes[start..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + off);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn resolve_var(body: &str, depth: u8) -> String {
    if let Some((name, default)) = body.split_once(":-") {
        // 默认值可含嵌套占位符（`${A:-${B:-x}}`），name 未设时递归展开 default 才
        // 能让 B 等内层变量真正生效；否则字面 `${B:-x}` 会原样落进 YAML 值。
        // depth 透传守 `EXPAND_ENV_MAX_DEPTH` 上限防递归爆栈。
        match env::var(name) {
            Ok(v) => sanitize_env_value(name, v),
            Err(_) => expand_env_depth(default, depth),
        }
    } else if let Ok(v) = env::var(body) {
        // 无 `:-` 默认值的 bare `${VAR}` 在 env 未设时返空串：YAML 接受空字符串
        // 值，sanitize 只对 archive_template / log.level 等 fields 兜底，其他 string
        // 字段会静默吃下空串（如 backend.smb.default_user）。改返 warn 让运维可见
        // 配置漂移；行为仍兼容（保留旧空串语义）。
        sanitize_env_value(body, v)
    } else {
        warn!(
            feature = "config",
            operation = "expand_env",
            result = "unset_var_empty_substitution",
            var = body,
            "placeholder var unset without default; substituting empty string. Use ':-default' suffix to silence."
        );
        String::new()
    }
}

/// 剥换行 / 回车 / NUL 等 yaml 结构性字符；env value 原文直接拼回 yaml 文本前的净化。
///
/// 攻击场景：`export TIDYMEDIA_LOG_LEVEL=$'info\narchive_template: "wrong/{year}"'`
/// 让换行注入新的顶层 yaml key 覆盖原 `archive_template`。yaml 1.2 的 plain/quoted
/// scalar 都把 LF/CR 视为分隔符或需 escape；最简单的兜底是直接丢弃这类字节并 warn。
/// 制表符 (`\t`) 保留——yaml plain scalar 允许且常见配置写法。
fn sanitize_env_value(var: &str, value: String) -> String {
    if value.bytes().any(yaml_unsafe_byte) {
        let cleaned: String = value.chars().filter(|c| !yaml_unsafe_char(*c)).collect();
        warn!(
            feature = "config",
            operation = "expand_env",
            result = "stripped_unsafe_bytes",
            var,
            "env value contains newline/control bytes; stripping to prevent yaml injection"
        );
        cleaned
    } else {
        value
    }
}

fn yaml_unsafe_byte(b: u8) -> bool {
    if matches!(b, b'\n' | b'\r' | 0) {
        return true;
    }
    if b >= 0x20 {
        return false;
    }
    b != b'\t'
}

fn yaml_unsafe_char(c: char) -> bool {
    if matches!(c, '\n' | '\r' | '\0') {
        return true;
    }
    if !c.is_control() {
        return false;
    }
    c != '\t'
}
