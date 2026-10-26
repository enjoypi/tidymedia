use tracing::warn;

use super::config_io;

const EXPAND_ENV_MAX_DEPTH: u8 = 32;

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
        match config_io::env_var(name) {
            Ok(v) => sanitize_env_value(name, v),
            Err(_) => expand_env_depth(default, depth),
        }
    } else if let Ok(v) = config_io::env_var(body) {
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
