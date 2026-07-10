//! move 半态标记错误：dst 已完整写入但 src 删除失败。
//!
//! 「copied ... but cannot remove source」曾靠字符串 `contains` 检测（`do_copy`
//! fast-path 救援分支），refactor 改措辞即静默失效 → 半态 dst 不入索引 → 重跑写
//! 重复副本。改为 `io::Error` 内嵌本标记类型，构造与检测共享单一契约；Display
//! 文案保持原样，既有「文案 MUST 含 copied ... but cannot remove source」断言不变。

use std::error::Error;
use std::fmt;
use std::io;

/// `io::Error` 的结构化 payload：标记「copy 已落地、remove source 失败」半态。
#[derive(Debug)]
pub struct PartialMove(String);

impl fmt::Display for PartialMove {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for PartialMove {}

/// 构造携带半态标记的 `io::Error`；`msg` 即对外 Display 文案。
pub fn partial_move_error(kind: io::ErrorKind, msg: String) -> io::Error {
    io::Error::new(kind, PartialMove(msg))
}

/// 判定 `e` 是否为 [`partial_move_error`] 构造的半态错误。
#[must_use]
pub fn is_partial_move(e: &io::Error) -> bool {
    matches!(e.get_ref(), Some(inner) if inner.is::<PartialMove>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_move_error_is_detected() {
        let e = partial_move_error(
            io::ErrorKind::PermissionDenied,
            "copied a -> b but cannot remove source: denied".to_string(),
        );
        assert!(is_partial_move(&e));
    }

    #[test]
    fn partial_move_error_keeps_kind_and_message() {
        let e = partial_move_error(io::ErrorKind::PermissionDenied, "msg text".to_string());
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(e.to_string(), "msg text");
    }

    #[test]
    fn plain_io_error_is_not_partial_move() {
        let e = io::Error::other("cannot remove source lookalike");
        assert!(!is_partial_move(&e));
    }

    #[test]
    fn errorkind_only_error_is_not_partial_move() {
        let e = io::Error::from(io::ErrorKind::NotFound);
        assert!(!is_partial_move(&e));
    }
}
