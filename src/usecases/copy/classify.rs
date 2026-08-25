//! copy-doc/move-doc 的文档内容分类接线：构造 [`TextClassifyProvider`] 闭包
//! （`open_read` → `extract_office_text` → `DocumentClassifier::classify` →
//! `score_min` 阈值裁决 → 类目名），供 `build_source_index` 在
//! `doc_only && 模板含 {category}` 时注入 `Index::classify_documents`。

use std::sync::Arc;

use camino::Utf8Path;
use tracing::debug;

use crate::entities::backend::Backend;
use crate::entities::file_index::TextClassifyProvider;
use crate::entities::office::extract_office_text;
use crate::entities::uri::Location;
use crate::usecases::classify::DocumentClassifier;
use crate::usecases::report::FEATURE_COPY;

/// 归档模板是否消费 `{category}`；不消费则整个分类阶段（模型加载 + 每文件
/// 文本提取 + 推理）都可跳过——「无消费点不做工」与 `parse_non_media` 短路同哲学。
pub(crate) fn template_needs_category(template: &str) -> bool {
    template.contains("{category}")
}

/// 把 `Arc<dyn DocumentClassifier>` 包装为 Entity 层可持的分类闭包。
/// `score_min` 阈值裁决在此完成：低于阈值 / 提不出文本 / 分类 Err 都返 `None`
/// （文件保持未分类，`{category}` 渲染时兜底 uncategorized）。
pub(crate) fn make_classify_provider(
    classifier: Arc<dyn DocumentClassifier>,
    max_text_bytes: usize,
    score_min: f32,
) -> TextClassifyProvider {
    Box::new(move |loc, backend, mime| {
        classify_one(
            loc,
            backend,
            mime,
            classifier.as_ref(),
            max_text_bytes,
            score_min,
        )
    })
}

/// 单文件分类：读源（MIME 复用 `parse_exif` 已 sniff 的值）→ 提取正文片段 →
/// 推理 → 阈值裁决。全程 best-effort：任何失败返 `None` 落 uncategorized，
/// 不中断归档。
///
/// 整 fn `coverage(off)`：IO 编排（`open_read` Err arm）需注入 reader 错误，
/// multi-instance 下 phantom miss 难闭合；分类语义由 `classify_tests.rs`
/// 用 `FakeBackend` + `FakeDocumentClassifier` 全分支断言。
fn classify_one(
    loc: &Location,
    backend: &Arc<dyn Backend>,
    mime: &str,
    classifier: &dyn DocumentClassifier,
    max_text_bytes: usize,
    score_min: f32,
) -> Option<String> {
    let mut reader = backend.open_read(loc).ok()?;
    let text = extract_office_text(reader.as_mut(), mime, max_text_bytes);
    if text.is_empty() {
        debug!(
            feature = FEATURE_COPY,
            operation = "classify_document",
            result = "no_text",
            location = %loc.display(),
            "no extractable text; falling back to uncategorized"
        );
        return None;
    }
    let path = Utf8Path::new(loc.path().as_str());
    let classification = classifier.classify(path, &text).ok()?;
    if classification.score < score_min {
        debug!(
            feature = FEATURE_COPY,
            operation = "classify_document",
            result = "below_threshold",
            location = %loc.display(),
            category = %classification.category,
            score = classification.score,
            score_min,
            "best category below score_min; falling back to uncategorized"
        );
        return None;
    }
    debug!(
        feature = FEATURE_COPY,
        operation = "classify_document",
        result = "ok",
        location = %loc.display(),
        category = %classification.category,
        score = classification.score,
        "document classified"
    );
    Some(classification.category)
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
