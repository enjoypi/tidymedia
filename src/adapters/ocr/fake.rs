//! 测试用 `TextDetector` 实现：路径查表 + 路径级 Err 注入。
//!
//! 设计与 `FakeBackend::inject_reader_error` 一致——键是 `Utf8PathBuf`，遍历顺序
//! 无关，断言精确。`default` 决定查表 miss 时的返回值，避免每条 fixture 都显式 `.with_result`。

use std::collections::{HashMap, HashSet};
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use parking_lot::Mutex;

use crate::usecases::ocr::TextDetector;

/// 路径查表 + Err 注入。`new(default)` 设定查表 miss 的返回值；多次 `.with_result`
/// 链式添加；`.inject_error(path)` 标记该路径返 Err。`Mutex` 让 trait 对象在
/// 多线程访问下保持安全（且让 `inject_error` 在测试期可后置追加）。
pub struct FakeTextDetector {
    results: Mutex<HashMap<Utf8PathBuf, bool>>,
    errors: Mutex<HashSet<Utf8PathBuf>>,
    default: bool,
}

impl FakeTextDetector {
    #[must_use]
    pub fn new(default: bool) -> Self {
        Self {
            results: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashSet::new()),
            default,
        }
    }

    /// 注入「该路径返指定 bool」。链式风格，构造时一次性预设。
    #[must_use]
    pub fn with_result(self, path: impl Into<Utf8PathBuf>, has_text: bool) -> Self {
        self.results.lock().insert(path.into(), has_text);
        self
    }

    /// 注入「该路径返 Err」，优先级高于 `with_result`。
    #[must_use]
    pub fn with_error(self, path: impl Into<Utf8PathBuf>) -> Self {
        self.errors.lock().insert(path.into());
        self
    }
}

impl std::fmt::Debug for FakeTextDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeTextDetector")
            .field("default", &self.default)
            .field("results_count", &self.results.lock().len())
            .field("errors_count", &self.errors.lock().len())
            .finish()
    }
}

impl TextDetector for FakeTextDetector {
    fn has_text(&self, path: &Utf8Path, _image_bytes: &[u8]) -> io::Result<bool> {
        if self.errors.lock().contains(path) {
            return Err(io::Error::other(format!(
                "FakeTextDetector: injected error for {path}"
            )));
        }
        Ok(self
            .results
            .lock()
            .get(path)
            .copied()
            .unwrap_or(self.default))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_returns_default_on_miss() {
        let d = FakeTextDetector::new(false);
        assert!(!d.has_text(Utf8Path::new("/x.png"), b"").unwrap());

        let d = FakeTextDetector::new(true);
        assert!(d.has_text(Utf8Path::new("/x.png"), b"").unwrap());
    }

    #[test]
    fn fake_returns_explicit_result() {
        let d = FakeTextDetector::new(false).with_result("/a.png", true);
        assert!(d.has_text(Utf8Path::new("/a.png"), b"").unwrap());
        assert!(!d.has_text(Utf8Path::new("/b.png"), b"").unwrap());
    }

    #[test]
    fn fake_injected_error_takes_precedence_over_result() {
        let d = FakeTextDetector::new(true)
            .with_result("/err.png", true)
            .with_error("/err.png");
        let e = d.has_text(Utf8Path::new("/err.png"), b"").unwrap_err();
        assert!(e.to_string().contains("injected error"));
    }

    #[test]
    fn fake_debug_redacts_internal_maps() {
        let d = FakeTextDetector::new(true)
            .with_result("/a", true)
            .with_error("/b");
        let s = format!("{d:?}");
        assert!(s.contains("FakeTextDetector"), "got: {s}");
        assert!(s.contains("results_count: 1"), "got: {s}");
        assert!(s.contains("errors_count: 1"), "got: {s}");
    }
}
