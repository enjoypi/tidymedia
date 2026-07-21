//! `doc_only`（copy-doc / move-doc）过滤路径测试：文档族放行、媒体与未知格式
//! skip。独立文件——`copy_advanced_tests.rs` 已超 512 行限。

#[cfg(test)]
mod test_doc_only {
    use std::path::Path;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::super::*;
    use crate::entities::test_common as tc;
    use crate::entities::uri::Location;

    const DEFAULT_TMPL: &str = "{year}/{month}/{valuable_name}";

    fn local_loc(p: &Path) -> Location {
        Location::Local(Utf8PathBuf::from(p.to_str().unwrap()))
    }

    fn doc_only_opts(template: &str) -> CopyOpts<'_> {
        CopyOpts {
            dry_run: false,
            remove: false,
            include_non_media: false,
            doc_only: true,
            template,
        }
    }

    // 用真实 PNG 字节做文件实体（hash 需要），Exif mime 决定 is_office/is_media 分流。
    fn info_with_mime(dir: &Path, name: &str, mime: &str) -> Info {
        let path = tc::copy_png_to(dir, name).unwrap();
        let mut info = Info::from(path.to_str().unwrap()).unwrap();
        info.set_exif(crate::entities::exif::Exif::with_mime(mime));
        info
    }

    #[test]
    fn do_copy_doc_only_copies_office_document() {
        let dir = tempdir().unwrap();
        let out = tempdir().unwrap();
        let info = info_with_mime(dir.path(), "report.pdf", "application/pdf");
        let idx = crate::entities::file_index::Index::new();
        let copied = do_copy(
            &info,
            &local_loc(out.path()),
            &crate::adapters::backend::local::LocalBackend::arc(),
            &idx,
            &doc_only_opts(DEFAULT_TMPL),
        )
        .unwrap();
        assert!(copied, "office document should pass doc_only filter");
    }

    #[test]
    fn do_copy_doc_only_skips_media_file() {
        let dir = tempdir().unwrap();
        let out = tempdir().unwrap();
        let info = info_with_mime(dir.path(), "photo.png", "image/png");
        let idx = crate::entities::file_index::Index::new();
        let copied = do_copy(
            &info,
            &local_loc(out.path()),
            &crate::adapters::backend::local::LocalBackend::arc(),
            &idx,
            &doc_only_opts(DEFAULT_TMPL),
        )
        .unwrap();
        assert!(!copied, "media file must be skipped under doc_only");
    }

    #[test]
    fn do_copy_doc_only_skips_unknown_binary() {
        let dir = tempdir().unwrap();
        let out = tempdir().unwrap();
        let info = info_with_mime(dir.path(), "blob.bin", "application/octet-stream");
        let idx = crate::entities::file_index::Index::new();
        let copied = do_copy(
            &info,
            &local_loc(out.path()),
            &crate::adapters::backend::local::LocalBackend::arc(),
            &idx,
            &doc_only_opts(DEFAULT_TMPL),
        )
        .unwrap();
        assert!(!copied, "unknown binary must be skipped under doc_only");
    }
}

#[cfg(test)]
mod test_resolved_template {
    use super::super::run::resolved_template;

    #[test]
    fn explicit_template_wins() {
        assert_eq!(resolved_template(Some("{year}"), true), "{year}");
    }

    #[test]
    fn doc_only_defaults_to_doc_archive_template() {
        crate::install_config_loader();
        assert_eq!(resolved_template(None, true), "{category}/{year}/{month}");
    }

    #[test]
    fn media_defaults_to_archive_template() {
        crate::install_config_loader();
        assert_eq!(
            resolved_template(None, false),
            "{year}/{month}/{valuable_name}"
        );
    }
}

#[cfg(test)]
mod test_classifier_gate {
    use std::path::Path;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::super::run::copy_with_sidecar;
    use crate::adapters::backend::local::LocalBackend;
    use crate::entities::backend::Backend;
    use crate::entities::file_index::TextClassifyProvider;
    use crate::entities::test_common as tc;
    use crate::entities::uri::Location;

    fn local_source(p: &Path) -> (Location, Arc<dyn Backend>) {
        (
            Location::Local(camino::Utf8PathBuf::from(p.to_str().unwrap())),
            LocalBackend::arc(),
        )
    }

    fn counting_provider(hits: Arc<std::sync::atomic::AtomicUsize>) -> TextClassifyProvider {
        Box::new(move |_, _, _| {
            hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some("hit".to_string())
        })
    }

    // doc_only + 模板含 {category} → classifier 被消费（filter True arm）。
    #[test]
    fn classifier_runs_when_doc_template_needs_category() {
        let src = tempdir().unwrap();
        std::fs::write(src.path().join("notes.txt"), b"body text here").unwrap();
        let out = tempdir().unwrap();
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        copy_with_sidecar(
            &[local_source(src.path())],
            local_source(out.path()),
            /* dry_run = */ true,
            /* remove = */ false,
            /* include_non_media = */ false,
            /* doc_only = */ true,
            Some("{category}/{year}"),
            None,
            None,
            Some(counting_provider(Arc::clone(&hits))),
        )
        .unwrap();
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "classifier must run for the txt document"
        );
    }

    // doc_only 但模板无 {category} → classifier 被 filter 丢弃（False arm），
    // 分类零调用（无消费点不做工）。
    #[test]
    fn classifier_skipped_when_template_has_no_category() {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "a.png").unwrap();
        std::fs::write(src.path().join("notes.txt"), b"body text here").unwrap();
        let out = tempdir().unwrap();
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        copy_with_sidecar(
            &[local_source(src.path())],
            local_source(out.path()),
            /* dry_run = */ true,
            /* remove = */ false,
            /* include_non_media = */ false,
            /* doc_only = */ true,
            Some("{year}/{month}"),
            None,
            None,
            Some(counting_provider(Arc::clone(&hits))),
        )
        .unwrap();
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "classifier must be dropped when template has no {{category}}"
        );
    }
}
