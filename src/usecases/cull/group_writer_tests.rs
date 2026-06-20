//! `group_writer` 单测：tests 外置以保 `group_writer.rs` ≤ 512 行（P0 §7）。

use super::*;
use crate::adapters::backend::factory::DefaultBackendFactory;
use crate::entities::backend::factory::BackendFactory;

fn local_loc(path: &str) -> Location {
Location::Local(camino::Utf8PathBuf::from(path))
}

#[test]
fn compute_group_dir_uses_relative_path() {
    let best = local_loc("/src/2024/05/IMG_001.jpg");
    let root = local_loc("/src");
    let output = local_loc("/out");
    let group = compute_group_dir(&best, &root, &output, 1);
    assert_eq!(group.path().as_str(), "/out/2024/05/group-001");
}

#[test]
fn compute_group_dir_handles_root_level_file() {
    let best = local_loc("/src/IMG.jpg");
    let root = local_loc("/src");
    let output = local_loc("/out");
    let group = compute_group_dir(&best, &root, &output, 7);
    assert_eq!(group.path().as_str(), "/out/group-007");
}

#[test]
fn compute_group_dir_pads_id_to_three_digits() {
    let best = local_loc("/src/IMG.jpg");
    let root = local_loc("/src");
    let output = local_loc("/out");
    let group = compute_group_dir(&best, &root, &output, 42);
    assert!(group.path().as_str().ends_with("group-042"));
}

#[test]
fn split_stem_ext_handles_extensions() {
    assert_eq!(split_stem_ext("IMG.jpg"), ("IMG", "jpg"));
    assert_eq!(split_stem_ext("no-ext"), ("no-ext", ""));
    assert_eq!(split_stem_ext(".hidden"), (".hidden", ""));
    assert_eq!(split_stem_ext("dotted.tar.gz"), ("dotted.tar", "gz"));
    // 末尾 '.'：rsplit_once 返 Some(("abc","")) → ext 空 → 走 _ arm
    assert_eq!(split_stem_ext("abc."), ("abc.", ""));
}

#[test]
fn write_group_dry_run_does_not_create_files() {
    let tmp = tempfile::tempdir().unwrap();
    let src_path = tmp.path().join("a.jpg");
    std::fs::write(&src_path, b"fake").unwrap();
    let src_loc = local_loc(src_path.to_str().unwrap());
    let root_loc = local_loc(tmp.path().to_str().unwrap());
    let output_path = tmp.path().join("out");
    let output_loc = local_loc(output_path.to_str().unwrap());
    let factory = DefaultBackendFactory;
    let backend = factory.for_location(&output_loc).unwrap();

    let plan = GroupPlan {
        group_id: 1,
        best_source: &src_loc,
        best_source_backend: &backend,
        culled: vec![],
        best_score: 100.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let report =
        write_group(&plan, &root_loc, &output_loc, &backend, true, &mut moved).unwrap();
    assert_eq!(report.group_id, 1);
    assert!(report.best_dest.contains("BEST_a.jpg"));
    assert_eq!(moved, 0);
    // dry_run 不创建任何 output 目录
    assert!(
        !output_path.exists(),
        "output dir should not exist in dry_run"
    );
}

use crate::adapters::backend::fake::{FakeBackend, Op};

fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "x".into(),
        path: camino::Utf8PathBuf::from(path),
    }
}

#[test]
fn write_group_non_dry_run_copies_best_and_moves_culled() {
    // 全 local：写 best 文件 + 两张 culled，验证 copy + rename + manifest 都落盘
    let tmp = tempfile::tempdir().unwrap();
    let best_path = tmp.path().join("best.jpg");
    std::fs::write(&best_path, b"BEST").unwrap();
    let culled_a = tmp.path().join("a.jpg");
    std::fs::write(&culled_a, b"AAA").unwrap();
    let culled_b = tmp.path().join("b.jpg");
    std::fs::write(&culled_b, b"BBB").unwrap();
    let best_loc = local_loc(best_path.to_str().unwrap());
    let first_culled = local_loc(culled_a.to_str().unwrap());
    let second_culled = local_loc(culled_b.to_str().unwrap());
    let root_loc = local_loc(tmp.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out_loc = local_loc(out_dir.path().to_str().unwrap());
    let factory = DefaultBackendFactory;
    let backend = factory.for_location(&out_loc).unwrap();
    let plan = GroupPlan {
        group_id: 3,
        best_source: &best_loc,
        best_source_backend: &backend,
        culled: vec![
            (&first_culled, &backend, 50.0),
            (&second_culled, &backend, 30.0),
        ],
        best_score: 99.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let report = write_group(&plan, &root_loc, &out_loc, &backend, false, &mut moved).unwrap();
    assert_eq!(report.culled.len(), 2);
    assert_eq!(moved, 2);
    let group_dir = out_dir.path().join("group-003");
    assert!(group_dir.join("BEST_best.jpg").exists());
    assert!(group_dir.join("a.jpg").exists());
    assert!(group_dir.join("b.jpg").exists());
    assert!(group_dir.join("MANIFEST.json").exists());
    // 源文件 culled 被 rename 走 → 原路径不存在
    assert!(!culled_a.exists());
}

#[test]
fn write_group_propagates_mkdir_p_failure() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    let group_dir_loc = smb_loc("/out/group-001");
    fake.inject_error(group_dir_loc, Op::MkdirP, io::ErrorKind::PermissionDenied);
    let backend: Arc<dyn Backend> = fake;
    let best = smb_loc("/src/x.jpg");
    let root = smb_loc("/src");
    let plan = GroupPlan {
        group_id: 1,
        best_source: &best,
        best_source_backend: &backend,
        culled: vec![],
        best_score: 1.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let err = write_group(&plan, &root, &out, &backend, false, &mut moved).unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}

#[test]
fn write_group_propagates_copy_failure() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    let src = smb_loc("/src/x.jpg");
    fake.add_dir(out.clone());
    fake.add_file(src.clone(), b"x".to_vec());
    // FakeBackend.copy_file 内 check_error 按 src loc 匹配
    fake.inject_error(src.clone(), Op::CopyFile, io::ErrorKind::PermissionDenied);
    let backend: Arc<dyn Backend> = fake;
    let root = smb_loc("/src");
    let plan = GroupPlan {
        group_id: 1,
        best_source: &src,
        best_source_backend: &backend,
        culled: vec![],
        best_score: 1.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let err = write_group(&plan, &root, &out, &backend, false, &mut moved).unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}

#[test]
fn write_group_propagates_best_unique_name_failure() {
    // unique_name_in_dir 内 backend.exists Err → 上抛覆盖 line 57 + 139
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    let src = smb_loc("/src/x.jpg");
    let candidate = smb_loc("/out/group-001/BEST_x.jpg");
    fake.add_dir(out.clone());
    fake.add_file(src.clone(), b"x".to_vec());
    fake.inject_error(candidate, Op::Exists, io::ErrorKind::PermissionDenied);
    let backend: Arc<dyn Backend> = fake;
    let root = smb_loc("/src");
    let plan = GroupPlan {
        group_id: 1,
        best_source: &src,
        best_source_backend: &backend,
        culled: vec![],
        best_score: 1.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let err = write_group(&plan, &root, &out, &backend, false, &mut moved).unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}

#[test]
fn write_group_propagates_culled_unique_name_failure() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    let best = smb_loc("/src/best.jpg");
    let culled = smb_loc("/src/culled.jpg");
    fake.add_dir(out.clone());
    fake.add_file(best.clone(), b"BEST".to_vec());
    fake.add_file(culled.clone(), b"CULL".to_vec());
    // best 的 unique 通过 → 触发 line 69（culled 的 unique_name）失败
    let culled_candidate = smb_loc("/out/group-001/culled.jpg");
    fake.inject_error(
        culled_candidate,
        Op::Exists,
        io::ErrorKind::PermissionDenied,
    );
    let backend: Arc<dyn Backend> = fake;
    let root = smb_loc("/src");
    let plan = GroupPlan {
        group_id: 1,
        best_source: &best,
        best_source_backend: &backend,
        culled: vec![(&culled, &backend, 1.0)],
        best_score: 2.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let err = write_group(&plan, &root, &out, &backend, false, &mut moved).unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}

#[test]
fn write_group_propagates_move_file_failure_for_culled() {
    // best copy OK + culled unique OK → move_file 调 copy_file，culled 的 CopyFile inject Err
    let fake = Arc::new(FakeBackend::new("smb"));
    let out = smb_loc("/out");
    let best = smb_loc("/src/best.jpg");
    let culled = smb_loc("/src/culled.jpg");
    fake.add_dir(out.clone());
    fake.add_file(best.clone(), b"BEST".to_vec());
    fake.add_file(culled.clone(), b"CULL".to_vec());
    // FakeBackend.copy_file 按 src loc 匹配 — culled 是 move_file 的 src
    fake.inject_error(
        culled.clone(),
        Op::CopyFile,
        io::ErrorKind::PermissionDenied,
    );
    let backend: Arc<dyn Backend> = fake;
    let root = smb_loc("/src");
    let plan = GroupPlan {
        group_id: 1,
        best_source: &best,
        best_source_backend: &backend,
        culled: vec![(&culled, &backend, 1.0)],
        best_score: 2.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let err = write_group(&plan, &root, &out, &backend, false, &mut moved).unwrap_err();
    assert!(err.to_string().contains("injected"), "got: {err}");
}

#[test]
fn write_group_rejects_best_source_without_file_name() {
    // path = "/" 的 file_name 是 None
    let tmp = tempfile::tempdir().unwrap();
    let root_loc = local_loc(tmp.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out_loc = local_loc(out_dir.path().to_str().unwrap());
    let factory = DefaultBackendFactory;
    let backend = factory.for_location(&out_loc).unwrap();
    let weird_best = local_loc("/");
    let plan = GroupPlan {
        group_id: 1,
        best_source: &weird_best,
        best_source_backend: &backend,
        culled: vec![],
        best_score: 1.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let err = write_group(&plan, &root_loc, &out_loc, &backend, true, &mut moved).unwrap_err();
    assert!(
        err.to_string().contains("best source has no file name"),
        "got: {err}"
    );
}

#[test]
fn write_group_rejects_culled_source_without_file_name() {
    let tmp = tempfile::tempdir().unwrap();
    let best_path = tmp.path().join("a.jpg");
    std::fs::write(&best_path, b"x").unwrap();
    let best_loc = local_loc(best_path.to_str().unwrap());
    let weird_culled = local_loc("/");
    let root_loc = local_loc(tmp.path().to_str().unwrap());
    let out_dir = tempfile::tempdir().unwrap();
    let out_loc = local_loc(out_dir.path().to_str().unwrap());
    let factory = DefaultBackendFactory;
    let backend = factory.for_location(&out_loc).unwrap();
    let plan = GroupPlan {
        group_id: 1,
        best_source: &best_loc,
        best_source_backend: &backend,
        culled: vec![(&weird_culled, &backend, 1.0)],
        best_score: 1.0,
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    let mut moved = 0;
    let err = write_group(&plan, &root_loc, &out_loc, &backend, true, &mut moved).unwrap_err();
    assert!(
        err.to_string().contains("culled source has no file name"),
        "got: {err}"
    );
}

#[test]
fn unique_name_in_dir_returns_base_when_dry_run() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let backend: Arc<dyn Backend> = fake;
    let dir = smb_loc("/d");
    let loc = unique_name_in_dir(&dir, "a.jpg", &backend, true).unwrap();
    assert_eq!(loc.path().as_str(), "/d/a.jpg");
}

#[test]
fn unique_name_in_dir_loops_when_base_exists() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let dir = smb_loc("/d");
    // 让 a.jpg 与 a_1.jpg 都已存在
    fake.add_file(smb_loc("/d/a.jpg"), b"x".to_vec());
    fake.add_file(smb_loc("/d/a_1.jpg"), b"x".to_vec());
    let backend: Arc<dyn Backend> = fake;
    let loc = unique_name_in_dir(&dir, "a.jpg", &backend, false).unwrap();
    assert_eq!(loc.path().as_str(), "/d/a_2.jpg");
}

#[test]
fn unique_name_in_dir_handles_extensionless_name() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let dir = smb_loc("/d");
    fake.add_file(smb_loc("/d/IMG"), b"x".to_vec());
    let backend: Arc<dyn Backend> = fake;
    let loc = unique_name_in_dir(&dir, "IMG", &backend, false).unwrap();
    assert_eq!(loc.path().as_str(), "/d/IMG_1");
}

#[test]
fn unique_name_in_dir_errors_when_exhausted() {
    // 占满 a.jpg + a_1.jpg .. a_N.jpg（N = unique_name_max_attempts，默认 10）
    // 所有 N+1 个候选都 exists → 退出循环返 exhausted Err
    let fake = Arc::new(FakeBackend::new("smb"));
    let max = config().copy.unique_name_max_attempts;
    let dir = smb_loc("/d");
    fake.add_file(smb_loc("/d/a.jpg"), b"x".to_vec());
    for i in 1..=max {
        fake.add_file(smb_loc(&format!("/d/a_{i}.jpg")), b"x".to_vec());
    }
    let backend: Arc<dyn Backend> = fake;
    let err = unique_name_in_dir(&dir, "a.jpg", &backend, false).unwrap_err();
    assert!(err.to_string().contains("exhausted"), "got: {err}");
}

#[test]
fn move_file_non_local_same_backend_copies_then_removes() {
    // src + output 同一 fake smb backend：scheme=="smb" 相等但 != "local"
    // → 走 cross-scheme copy + remove 分支（line 162 short-circuit false）
    let fake = Arc::new(FakeBackend::new("smb"));
    let src_loc = smb_loc("/x.jpg");
    let target = smb_loc("/out/x.jpg");
    fake.add_file(src_loc.clone(), b"DATA".to_vec());
    let backend: Arc<dyn Backend> = fake.clone();
    move_file(&backend, &src_loc, &backend, &target).unwrap();
    assert!(fake.exists(&target).unwrap());
    assert!(!fake.exists(&src_loc).unwrap(), "src removed after move");
}

#[test]
fn move_file_different_scheme_takes_cross_scheme_branch() {
    // src_backend.scheme()="local", output_backend.scheme()="smb" 不同 →
    // line 162 第一个 == 短路 false → 走 cross-scheme copy + remove
    let src_fake = Arc::new(FakeBackend::new("local"));
    let out_fake = Arc::new(FakeBackend::new("smb"));
    let src_loc = smb_loc("/src/x.jpg");
    let target = smb_loc("/out/x.jpg");
    src_fake.add_file(src_loc.clone(), b"DATA".to_vec());
    // out_fake.copy_file 内 check src in own state → 也 add
    out_fake.add_file(src_loc.clone(), b"DATA".to_vec());
    let src_backend: Arc<dyn Backend> = src_fake.clone();
    let out_backend: Arc<dyn Backend> = out_fake;
    move_file(&src_backend, &src_loc, &out_backend, &target).unwrap();
    assert!(!src_fake.exists(&src_loc).unwrap());
}

#[test]
fn move_file_non_local_propagates_remove_failure() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let src_loc = smb_loc("/x.jpg");
    let target = smb_loc("/out/x.jpg");
    fake.add_file(src_loc.clone(), b"DATA".to_vec());
    fake.inject_error(
        src_loc.clone(),
        Op::RemoveFile,
        io::ErrorKind::PermissionDenied,
    );
    let backend: Arc<dyn Backend> = fake;
    let err = move_file(&backend, &src_loc, &backend, &target).unwrap_err();
    assert!(
        err.to_string().contains("but cannot remove source"),
        "got: {err}"
    );
}

#[test]
fn write_manifest_warns_on_open_write_failure() {
    // open_write 失败 → warn 路径（不返 Err，函数返 ()）
    let fake = Arc::new(FakeBackend::new("smb"));
    let group_dir = smb_loc("/out/group-001");
    let manifest_loc = smb_loc("/out/group-001/MANIFEST.json");
    fake.inject_error(manifest_loc, Op::OpenWrite, io::ErrorKind::PermissionDenied);
    let backend: Arc<dyn Backend> = fake;
    let report = super::super::report::GroupReport {
        group_id: 1,
        best_source: "/src/x.jpg".into(),
        best_dest: "/out/group-001/BEST_x.jpg".into(),
        culled: vec![],
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    // 不 panic 即成功
    write_manifest(&group_dir, &backend, &report, 1.0);
}

#[test]
fn write_manifest_warns_on_write_failure() {
    let fake = Arc::new(FakeBackend::new("smb"));
    let group_dir = smb_loc("/out/group-001");
    let manifest_loc = smb_loc("/out/group-001/MANIFEST.json");
    // open_write 成功但 write_all 失败
    fake.inject_writer_error(manifest_loc, io::ErrorKind::PermissionDenied);
    let backend: Arc<dyn Backend> = fake;
    let report = super::super::report::GroupReport {
        group_id: 1,
        best_source: "/src/x.jpg".into(),
        best_dest: "/out/group-001/BEST_x.jpg".into(),
        culled: vec![],
        score_breakdown: super::super::report::ScoreBreakdown::default(),
    };
    write_manifest(&group_dir, &backend, &report, 1.0);
}

