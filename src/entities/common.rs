use thiserror::Error;

use crate::entities::uri::Location;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error occurred: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// 判 path 是否在 prefix 目录下（或恰等于 prefix）：纯 `starts_with` 会让
/// `/photos_backup` 误判为 `/photos` 子目录，必须额外校验 prefix 后紧跟路径
/// 分隔符。Unix 用 `/`，Windows 兼顾 `\`。
/// 调用方传 prefix 末尾若已含分隔符（用户带尾斜杠 / dry-run 下原始字符串保留），
/// 内部剥掉避免 `rest` 既非空又不以分隔符开头的盲区误判。
#[must_use]
pub fn under_prefix(path: &str, prefix: &str) -> bool {
    let prefix = prefix.strip_suffix(['/', '\\']).unwrap_or(prefix);
    if !path.starts_with(prefix) {
        return false;
    }
    let rest = &path[prefix.len()..];
    rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')
}

/// 把 [`Location`] 规范化为 prefix 字符串：Local 路径 canonicalize（解析符号
/// 链接 + 相对路径转绝对）；远端 backend 直接 display。copy / move / cull /
/// move-text-shot 4 个 use case 的「source 是否在 output 子树」判定共用此助手——
/// 朴素 `Location::display()` 在 output 是符号链接时（`/tmp/out → /photos/out`）
/// 会让 src `/photos/out/img.jpg` 与 output prefix `/tmp/out` 字面不匹配，
/// `under_prefix` 误返 false，move 模式下源被当成"output 外"被搬迁致循环或丢失。
#[must_use]
pub fn canonical_prefix(loc: &Location) -> String {
    match loc {
        Location::Local(p) => match crate::entities::file_info::full_path(p.as_str()) {
            Ok(fp) => fp.as_str().to_string(),
            Err(_) => p.as_str().to_string(),
        },
        other => other.display(),
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn io_error_display_contains_inner_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err: Error = io_err.into();
        let msg = format!("{err}");
        assert!(msg.starts_with("IO error occurred:"), "got: {msg}");
        assert!(msg.contains("no such file"), "got: {msg}");
    }
}
