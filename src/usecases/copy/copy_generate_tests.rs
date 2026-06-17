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
        template,
    }
}

#[test]
fn generate_unique_name_uses_suffix_when_first_taken() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    let out_utf8 = utf8(out.path());
    let sub = out.path().join("2024").join("01");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("photo.png"), b"x").unwrap();
    let _ = out_utf8;
    let (_, target) =
        generate_unique_name(&info, &local_loc(out.path()), &local_arc(), DEFAULT_TMPL)
            .unwrap()
            .expect("unique name should be generated");
    let target_str = target.display();
    assert!(target_str.ends_with("photo_1.png"), "got {target_str}");
}

#[test]
fn generate_unique_name_none_after_max_collisions() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    fill_collisions(&out.path().join("2024").join("01"));
    let res =
        generate_unique_name(&info, &local_loc(out.path()), &local_arc(), DEFAULT_TMPL).unwrap();
    assert!(
        res.is_none(),
        "should exhaust after max_attempts+1 collisions"
    );
}

// 无扩展名文件冲突重命名不得产生尾点（"photo_1." 在 Windows 会被 CreateFile 剥点、
// Linux 下是不可见怪文件）。
#[test]
fn generate_unique_name_no_extension_omits_trailing_dot() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo");
    let out = tempdir().unwrap();
    let sub = out.path().join("2024").join("01");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("photo"), b"x").unwrap();
    let (_, target) =
        generate_unique_name(&info, &local_loc(out.path()), &local_arc(), DEFAULT_TMPL)
            .unwrap()
            .expect("unique name should be generated");
    let target_str = target.display();
    assert!(target_str.ends_with("photo_1"), "got {target_str}");
}

// stream_copy 中途失败（reader read Err）须清理 open_write 已创建的半截目标文件。
#[test]
fn do_copy_stream_failure_removes_partial_target() {
    let fake = Arc::new(crate::FakeBackend::new("local"));
    let src_loc = Location::Local(Utf8PathBuf::from("/src/photo.png"));
    fake.add_file(src_loc.clone(), b"data".to_vec());
    let mut info = Info::open(&src_loc, Arc::clone(&fake) as Arc<dyn Backend>).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    // Info::open 已读完 fast hash；此后注入让 stream_copy 的 io::copy 阶段失败。
    fake.inject_reader_error(src_loc, std::io::ErrorKind::TimedOut);

    let out = tempdir().unwrap();
    let mut idx = crate::entities::file_index::Index::new();
    let err = do_copy(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        &mut idx,
        &default_opts(DEFAULT_TMPL),
    )
    .expect_err("stream copy must fail");
    assert!(err.to_string().contains("IO error"), "got: {err}");
    // FakeBackend 默认 mtime = UNIX_EPOCH → +8h → 1970/01；半截目标必须被清理。
    let partial = out.path().join("1970").join("01").join("photo.png");
    assert!(!partial.exists(), "partial target must be cleaned up");
}

// stream_copy 内 `src_be.open_read(src.location())?` Err arm：Info::open 完成后注入
// Op::OpenRead Err，让 stream_copy 第二次调 open_read 时整体失败（区别于
// inject_reader_error 让 read 阶段失败、open_read 自身仍成功）。本测试钉 lib unit
// instance 的 stream_copy L120 `?` Err region 命中。
#[test]
fn do_copy_propagates_stream_open_read_error() {
    let fake = Arc::new(crate::FakeBackend::new("local"));
    let src_loc = Location::Local(Utf8PathBuf::from("/src/photo.png"));
    fake.add_file(src_loc.clone(), b"data".to_vec());
    let mut info = Info::open(&src_loc, Arc::clone(&fake) as Arc<dyn Backend>).unwrap();
    info.set_exif(crate::entities::exif::Exif::with_mime("image/png"));
    // Info::open 已读完 fast hash + mime sniff；此后注入 OpenRead Err 让 stream_copy
    // 调 src.backend().open_read(src.location()) 时直接失败。
    fake.inject_error(
        src_loc,
        crate::FakeOp::OpenRead,
        std::io::ErrorKind::Interrupted,
    );

    let out = tempdir().unwrap();
    let mut idx = crate::entities::file_index::Index::new();
    let err = do_copy(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        &mut idx,
        &default_opts(DEFAULT_TMPL),
    )
    .expect_err("stream_copy open_read must propagate Err");
    let _ = err;
}

// exists 的 IO 错误必须传播而非被当作"不存在"：吞错会让 stream_copy 覆盖已存在目标。
#[test]
fn generate_unique_name_propagates_exists_error() {
    let be = Arc::new(crate::FakeBackend::new("local"));
    let src_loc = Location::Local(Utf8PathBuf::from("/src/photo.png"));
    be.add_file(src_loc.clone(), b"data".to_vec());
    let info = Info::open(&src_loc, Arc::clone(&be) as Arc<dyn Backend>).unwrap();
    // FakeBackend 默认 mtime = UNIX_EPOCH → +8h 偏移 → 1970/01 子目录。
    let target = Location::Local(Utf8PathBuf::from("/out/1970/01/photo.png"));
    be.inject_error(target, crate::FakeOp::Exists, std::io::ErrorKind::TimedOut);
    let out_loc = Location::Local(Utf8PathBuf::from("/out"));
    let err =
        generate_unique_name(&info, &out_loc, &(be as Arc<dyn Backend>), DEFAULT_TMPL).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
}

#[test]
fn do_copy_errors_when_unique_name_exhausted() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    fill_collisions(&out.path().join("2024").join("01"));
    let mut idx = crate::entities::file_index::Index::new();
    let err = do_copy(
        &info,
        &local_loc(out.path()),
        &local_arc(),
        &mut idx,
        &default_opts(DEFAULT_TMPL),
    )
    .expect_err("must error after collisions");
    assert!(err.to_string().contains("无法为"));
}

#[test]
fn copy_logs_failure_when_target_collisions_exhausted() {
    let src = tempdir().unwrap();
    tc::copy_png_to(src.path(), "photo.png").unwrap();
    let out = tempdir().unwrap();
    fill_collisions(&out.path().join("2024").join("01"));
    copy(
        &[local_source(src.path())],
        local_source(out.path()),
        false,
        false,
        false,
        None,
        None,
    )
    .unwrap();
}

#[test]
fn do_copy_dry_run_reports_target_but_writes_nothing() {
    let src = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let out = tempdir().unwrap();
    let mut idx = crate::entities::file_index::Index::new();
    let opts = CopyOpts {
        dry_run: true,
        remove: false,
        include_non_media: false,
        template: DEFAULT_TMPL,
    };
    let did_copy = do_copy(&info, &local_loc(out.path()), &local_arc(), &mut idx, &opts).unwrap();
    assert!(did_copy);
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

// 启用 trace 级别 subscriber，让 copy() 里的 trace! 宏闭包被求值，覆盖 L62 region。
#[test]
fn copy_with_trace_subscriber_executes_trace_branch() {
    use tracing_subscriber::EnvFilter;
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("trace"))
        .with_writer(std::io::sink)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let src = tempdir().unwrap();
        tc::copy_png_to(src.path(), "photo.png").unwrap();
        let out = tempdir().unwrap();
        copy(
            &[local_source(src.path())],
            local_source(out.path()),
            true,
            false,
            false,
            None,
            None,
        )
        .unwrap();
    });
}

// `..` / `.` 路径段（恶意 EXIF 字段或自定义模板静态文本）必须被 naming::filter
// 过滤掉；Utf8PathBuf::join("..") 是字面拼接不规范化，让 fs::File::create 按字面
// 解析致 output 父目录被写入。本测试用自定义模板 `{year}/../target/.` 模拟该
// 注入向量，验证逐段剥除后 sub_dir 落在 out/<year>/target 而非 out 父目录。
#[test]
fn generate_unique_name_drops_dot_and_dotdot_segments() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    let info = make_media_info(src.path(), "photo.png");
    let template = "{year}/../target/.";
    let (sub_dir, target) =
        generate_unique_name(&info, &local_loc(out.path()), &local_arc(), template)
            .unwrap()
            .expect("path generated");
    let sub_dir_str = sub_dir.path().as_str();
    let out_str = utf8(out.path());
    assert!(
        sub_dir_str.starts_with(out_str.as_str()),
        "subdir {sub_dir_str} must stay under output {out_str}"
    );
    assert!(
        !sub_dir_str.contains(".."),
        "subdir must not contain dotdot, got: {sub_dir_str}"
    );
    assert!(target.path().as_str().starts_with(out_str.as_str()));
    assert!(target.path().as_str().ends_with("target/photo.png"));
}

// output 是已存在文件（非目录），backend.mkdir_p 失败 → 覆盖 copy() 内 `?` Err 分支。
#[test]
fn copy_with_output_as_file_errors() {
    let src = tempdir().unwrap();
    tc::copy_png_to(src.path(), "photo.png").unwrap();
    let out_file = tempfile::NamedTempFile::new().unwrap();
    let out_loc = Location::Local(Utf8PathBuf::from(out_file.path().to_str().unwrap()));
    let err = copy(
        &[local_source(src.path())],
        (out_loc, local_arc()),
        false,
        false,
        false,
        None,
        None,
    )
    .unwrap_err();
    let _ = err;
}
