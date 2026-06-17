//! group 目录写：算出 `output/<rel-dir>/group-NNN/` 路径，把最佳照片 `Backend::copy_file`
//! 复制为 `BEST_<basename>`，劣质副本 `Backend::rename` 搬到同目录，并写 `MANIFEST.json`。
//!
//! basename 冲突走 `unique_name_in_dir`（与 `move_text_shot` 同套路）。
//! MANIFEST.json 失败仅 warn 不中断（与 `JsonFileReportSink` 同哲学）。

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tracing::warn;

use super::report::GroupReport;
use crate::entities::backend::Backend;
use crate::entities::uri::Location;
use crate::usecases::config::config;

const FEATURE: &str = "cull";
const BEST_PREFIX: &str = "BEST_";
const MANIFEST_NAME: &str = "MANIFEST.json";

/// 单个相似组的搬迁计划：最佳源 + 全部劣质源（按评分降序）。
pub(crate) struct GroupPlan<'a> {
    pub group_id: usize,
    pub best_source: &'a Location,
    pub best_source_backend: &'a Arc<dyn Backend>,
    /// `(source_loc, source_backend, score)` 三元组。
    pub culled: Vec<(&'a Location, &'a Arc<dyn Backend>, f32)>,
    pub best_score: f32,
    pub score_breakdown: super::report::ScoreBreakdown,
}

/// 把 `plan` 落盘：返填好的 `GroupReport`。`dry_run` 时只算路径不真搬。
///
/// # Errors
///
/// `mkdir_p` 失败、`copy_file` 失败、`rename` 失败均上抛由 caller 计入 report.failed。
pub(crate) fn write_group(
    plan: &GroupPlan<'_>,
    source_root: &Location,
    output: &Location,
    output_backend: &Arc<dyn Backend>,
    dry_run: bool,
    moved_counter: &mut usize,
) -> io::Result<GroupReport> {
    let group_dir = compute_group_dir(plan.best_source, source_root, output, plan.group_id);
    if !dry_run {
        output_backend.mkdir_p(&group_dir)?;
    }

    let best_basename = plan
        .best_source
        .path()
        .file_name()
        .ok_or_else(|| io::Error::other("best source has no file name"))?;
    let best_dst_name = format!("{BEST_PREFIX}{best_basename}");
    let best_dst = unique_name_in_dir(&group_dir, &best_dst_name, output_backend, dry_run)?;
    if !dry_run
        && let Err(e) = plan
            .best_source_backend
            .copy_file(plan.best_source, &best_dst, false)
    {
        // copy_file 跨 scheme 不保证原子：部分字节落盘后 Err 残留半截 dst 文件，
        // 重跑时 unique_name 跳到 BEST_x_1 致重复堆积。best-effort 清理。
        let _ = output_backend.remove_file(&best_dst);
        return Err(e);
    }

    let mut culled_reports = Vec::with_capacity(plan.culled.len());
    for (src_loc, src_backend, score) in &plan.culled {
        let src_basename = src_loc
            .path()
            .file_name()
            .ok_or_else(|| io::Error::other("culled source has no file name"))?;
        let dst = unique_name_in_dir(&group_dir, src_basename, output_backend, dry_run)?;
        if !dry_run {
            move_file(src_backend, src_loc, output_backend, &dst)?;
            *moved_counter += 1;
        }
        culled_reports.push(super::report::CulledEntry {
            source_path: src_loc.display(),
            dest_path: dst.display(),
            score: *score,
        });
    }

    let report = GroupReport {
        group_id: plan.group_id,
        best_source: plan.best_source.display(),
        best_dest: best_dst.display(),
        culled: culled_reports,
        score_breakdown: plan.score_breakdown,
    };
    if !dry_run {
        write_manifest(&group_dir, output_backend, &report, plan.best_score);
    }
    Ok(report)
}

/// 算 `output/<best-rel-dir>/group-NNN/`。`best-rel-dir` 是最佳照片相对 source root 的目录。
fn compute_group_dir(
    best_source: &Location,
    source_root: &Location,
    output: &Location,
    group_id: usize,
) -> Location {
    let best_path = best_source.path();
    let rel_dir = best_path
        .strip_prefix(source_root.path())
        .ok()
        .and_then(camino::Utf8Path::parent)
        .map_or_else(Utf8PathBuf::new, Utf8PathBuf::from);
    let group_name = format!("group-{group_id:03}");
    let combined = if rel_dir.as_str().is_empty() {
        output.path().join(&group_name)
    } else {
        output.path().join(&rel_dir).join(&group_name)
    };
    output.with_path(combined)
}

/// basename 冲突走 `unique_name`：`a.jpg` 存在则 `a_1.jpg` / `a_2.jpg`。
/// `dry_run` 时直接返原 basename 不检 `exists`（避免 backend 调用）。
fn unique_name_in_dir(
    dir: &Location,
    file_name: &str,
    backend: &Arc<dyn Backend>,
    dry_run: bool,
) -> io::Result<Location> {
    let base_loc = dir.with_path(dir.path().join(file_name));
    if dry_run {
        return Ok(base_loc);
    }
    let (stem, ext) = split_stem_ext(file_name);
    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..=max_attempts {
        let candidate = if i == 0 {
            file_name.to_string()
        } else if ext.is_empty() {
            format!("{stem}_{i}")
        } else {
            format!("{stem}_{i}.{ext}")
        };
        let loc = dir.with_path(dir.path().join(&candidate));
        if !backend.exists(&loc)? {
            return Ok(loc);
        }
    }
    Err(io::Error::other(format!(
        "exhausted unique-name attempts in {}",
        dir.display()
    )))
}

fn split_stem_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

fn move_file(
    src_backend: &Arc<dyn Backend>,
    src_loc: &Location,
    output_backend: &Arc<dyn Backend>,
    target_loc: &Location,
) -> io::Result<()> {
    if src_backend.scheme() == output_backend.scheme() && src_backend.scheme() == "local" {
        return src_backend.rename(src_loc, target_loc, false);
    }
    // 跨 scheme：copy + remove（与 move_text_shot 同套路，半态错文案）
    output_backend.copy_file(src_loc, target_loc, false)?;
    src_backend.remove_file(src_loc).map_err(|re| {
        io::Error::new(
            re.kind(),
            format!(
                "cull: copied {src} -> {dst} but cannot remove source: {re}",
                src = src_loc.display(),
                dst = target_loc.display(),
            ),
        )
    })
}

fn write_manifest(
    group_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    report: &GroupReport,
    best_score: f32,
) {
    #[derive(serde_derive::Serialize)]
    struct Manifest<'a> {
        group_id: usize,
        best: BestEntry<'a>,
        culled: &'a [super::report::CulledEntry],
        score_breakdown: super::report::ScoreBreakdown,
    }
    #[derive(serde_derive::Serialize)]
    struct BestEntry<'a> {
        src: &'a str,
        dst: &'a str,
        score: f32,
    }
    let manifest_loc = group_dir.with_path(group_dir.path().join(MANIFEST_NAME));
    let m = Manifest {
        group_id: report.group_id,
        best: BestEntry {
            src: &report.best_source,
            dst: &report.best_dest,
            score: best_score,
        },
        culled: &report.culled,
        score_breakdown: report.score_breakdown,
    };
    // Manifest 是 plain struct，serde_json::to_vec_pretty 对其永不返 Err；
    // expect 标注 internal invariant 而非 defensive。
    let json = serde_json::to_vec_pretty(&m)
        .expect("internal: Manifest serialization never fails for plain structs");
    let mut writer = match output_backend.open_write(&manifest_loc, false) {
        Ok(w) => w,
        Err(e) => {
            warn!(
                feature = FEATURE,
                operation = "write_manifest",
                result = "open_error",
                error = %e,
                "MANIFEST.json open failed; group skips manifest"
            );
            return;
        }
    };
    if let Err(e) = writer.write_all(&json).and_then(|()| writer.finish()) {
        warn!(
            feature = FEATURE,
            operation = "write_manifest",
            result = "write_error",
            error = %e,
            "MANIFEST.json write failed"
        );
    }
}

#[cfg(test)]
mod tests {
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
}
