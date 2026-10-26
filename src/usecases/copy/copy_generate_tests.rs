//! `generate_unique_name` 冲突序号与 `do_copy` 异常 / trace / dry-run 路径测试
//!（从 `copy_tests.rs` 拆出）。

use std::fs;
use std::path::Path;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::*;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::file_index::Index;
use crate::entities::test_common as tc;
use crate::entities::uri::Location;

const DEFAULT_TMPL: &str = "{year}/{month}/{valuable_name}";

fn utf8(p: &Path) -> Utf8PathBuf {
    Utf8PathBuf::from(p.to_str().unwrap())
}

fn local_loc(p: &Path) -> Location {
    Location::Local(utf8(p))
}

fn local_arc() -> Arc<dyn Backend> {
    LocalBackend::arc()
}

fn local_source(p: &Path) -> (Location, Arc<dyn Backend>) {
    (local_loc(p), local_arc())
}

fn make_media_info(dir: &Path, name: &str) -> Info {
    let png = tc::copy_png_to(dir, name).unwrap();
    let mut info = Info::from(png.to_str().unwrap()).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    info
}

fn fill_collisions(sub: &Path) {
    fs::create_dir_all(sub).unwrap();
    fs::write(sub.join("photo.png"), b"").unwrap();
    // 与 naming.rs 的 `0..=max_attempts` 同步：max_attempts=10 时填原名 + _1..=_10
    // 共 11 个 slot 才能耗尽。
    for i in 1..=10 {
        fs::write(sub.join(format!("photo_{i}.png")), b"").unwrap();
    }
}

fn default_opts(template: &str) -> CopyOpts<'_> {
    CopyOpts {
        dry_run: false,
        remove: false,
        include_non_media: false,
        doc_only: false,
        template,
    }
}

include!("copy_generate_part1_tests.rs");
include!("copy_generate_part2_tests.rs");
