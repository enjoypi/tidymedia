//! 测试用 [`DocumentClassifier`] 实现：文本子串规则匹配 + 路径级 Err 注入。
//!
//! 设计与 `FakeTextDetector` 一致——`with_rule(needle, category, score)` 链式
//! 预设「文本含 needle → 判为 category」；`with_error(path)` 注入路径级 Err；
//! 无规则命中时返 `default_category` + 低分（0.0），配合 `score_min` 阈值
//! 自然落 uncategorized。

use std::collections::HashSet;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use parking_lot::Mutex;

use crate::usecases::classify::{Classification, DocumentClassifier};

/// 规则表 + Err 注入。规则按插入序匹配首个 `text.contains(needle)`。
pub struct FakeDocumentClassifier {
    rules: Mutex<Vec<(String, String, f32)>>,
    errors: Mutex<HashSet<Utf8PathBuf>>,
    default_category: String,
}

impl FakeDocumentClassifier {
    #[must_use]
    pub fn new(default_category: impl Into<String>) -> Self {
        Self {
            rules: Mutex::new(Vec::new()),
            errors: Mutex::new(HashSet::new()),
            default_category: default_category.into(),
        }
    }

    /// 注入「文本含 `needle` → 判为 `category`（相似度 `score`）」。链式预设。
    #[must_use]
    pub fn with_rule(
        self,
        needle: impl Into<String>,
        category: impl Into<String>,
        score: f32,
    ) -> Self {
        self.rules
            .lock()
            .push((needle.into(), category.into(), score));
        self
    }

    /// 注入「该路径返 Err」，优先级高于规则。
    #[must_use]
    pub fn with_error(self, path: impl Into<Utf8PathBuf>) -> Self {
        self.errors.lock().insert(path.into());
        self
    }
}

impl std::fmt::Debug for FakeDocumentClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeDocumentClassifier")
            .field("default_category", &self.default_category)
            .field("rules_count", &self.rules.lock().len())
            .field("errors_count", &self.errors.lock().len())
            .finish()
    }
}

impl DocumentClassifier for FakeDocumentClassifier {
    fn classify(&self, path: &Utf8Path, text: &str) -> io::Result<Classification> {
        if self.errors.lock().contains(path) {
            return Err(io::Error::other(format!(
                "FakeDocumentClassifier: injected error for {path}"
            )));
        }
        let rules = self.rules.lock();
        for (needle, category, score) in rules.iter() {
            if text.contains(needle.as_str()) {
                return Ok(Classification {
                    category: category.clone(),
                    score: *score,
                });
            }
        }
        Ok(Classification {
            category: self.default_category.clone(),
            score: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_matches_first_rule_by_substring() {
        let c = FakeDocumentClassifier::new("misc").with_rule("发票", "invoice", 0.9);
        let r = c
            .classify(Utf8Path::new("/a.pdf"), "增值税发票抬头")
            .unwrap();
        assert_eq!(r.category, "invoice");
    }

    #[test]
    fn fake_rule_score_passthrough() {
        let c = FakeDocumentClassifier::new("misc").with_rule("x", "cat", 0.7);
        let r = c.classify(Utf8Path::new("/a"), "x").unwrap();
        assert!((r.score - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn fake_miss_returns_default_with_low_score() {
        let c = FakeDocumentClassifier::new("misc").with_rule("发票", "invoice", 0.9);
        let r = c.classify(Utf8Path::new("/a.txt"), "无关内容").unwrap();
        assert_eq!(r.category, "misc");
        assert!(r.score.abs() < f32::EPSILON);
    }

    #[test]
    fn fake_injected_error_wins_over_rules() {
        let c = FakeDocumentClassifier::new("misc")
            .with_rule("x", "cat", 0.9)
            .with_error("/bad.doc");
        let err = c.classify(Utf8Path::new("/bad.doc"), "x").unwrap_err();
        assert!(err.to_string().contains("injected"), "got: {err}");
    }

    #[test]
    fn fake_debug_redacts_internals() {
        let c = FakeDocumentClassifier::new("misc").with_rule("a", "b", 0.5);
        let s = format!("{c:?}");
        assert!(s.contains("rules_count"), "got: {s}");
    }
}
