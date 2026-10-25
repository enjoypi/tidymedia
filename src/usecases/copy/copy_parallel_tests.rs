//! F1 `run_copy_loop` 分桶并行化正确性测试。
//!
//! - `run_copy_loop_parallel_correctness_deterministic`：10 unique + 5 dup 内容混合，
//!   重复运行让并行 winner 顺序稳定（Phase A 按 `fast_hash` 分桶 + 桶内字典序排序）
//! - `run_copy_loop_dedup_produces_single_winner_deterministically`：3 份同 hash 源
//!   在并行下必稳定归档 1 份 + `ignored` 2 份

#[cfg(test)]
mod test_parallel {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::super::png_fixture::write_unique_png;
    use super::super::*;
    use crate::adapters::backend::local::LocalBackend;
    use crate::entities::backend::Backend;
    use crate::entities::uri::Location;

    const DEFAULT_TMPL: &str = "{year}/{month}/{valuable_name}";

    fn local_source(p: &Path) -> (Location, Arc<dyn Backend>) {
        (
            Location::Local(Utf8PathBuf::from(p.to_str().unwrap())),
            LocalBackend::arc(),
        )
    }

    // 复制已生成的 unique png，让 fast_hash + secure_hash 精确等价源。
    fn dup_png(src: &Path, dst_dir: &Path, name: &str) -> std::path::PathBuf {
        let p = dst_dir.join(name);
        fs::copy(src, &p).unwrap();
        let ts =
            filetime::FileTime::from_unix_time(crate::entities::test_common::FIXED_MEDIA_MTIME, 0);
        filetime::set_file_mtime(&p, ts).unwrap();
        p
    }

    // F1：并行 run_copy_loop 处理 10 unique + 5 dup 混合源，报告项计数在多轮重复
    // 运行下必须完全一致（Phase A 分桶排序保证 dedup 顺序确定）。
    #[test]
    fn run_copy_loop_parallel_correctness_deterministic() {
        // 十次独立跑（out_dir 每次新建）；断言完全一致。20 次太慢，10 次足以暴露
        // 非确定性（并行调度差异下 5+ 轮必现单次分歧）。
        let mut all_reports: Vec<(usize, usize, usize)> = Vec::with_capacity(10);
        for _run in 0..10 {
            let src = tempdir().unwrap();
            let out = tempdir().unwrap();

            // 10 unique
            let mut uniq_paths = Vec::with_capacity(10);
            for i in 0..10u64 {
                let p = write_unique_png(src.path(), &format!("u_{i:02}.png"), i);
                uniq_paths.push(p);
            }
            // 5 dup（复制 unique[0..5] 各一份，让首 5 个 unique 都有对应 dup）
            for (i, unique_path) in uniq_paths.iter().take(5).enumerate() {
                dup_png(unique_path, src.path(), &format!("d_{i:02}.png"));
            }

            let report = copy(
                &[local_source(src.path())],
                local_source(out.path()),
                false,
                false,
                false,
                Some(DEFAULT_TMPL),
                None,
            )
            .unwrap();

            all_reports.push((report.copied, report.ignored, report.failed));
        }

        // 期望：10 unique 各 copied 一次；5 dup 命中 exists() 判 ignored；failed=0
        let want = (10usize, 5usize, 0usize);
        for (i, got) in all_reports.iter().enumerate() {
            assert_eq!(
                got, &want,
                "run #{i}: expected {want:?}, got {got:?} — 并行 dedup 非确定性"
            );
        }
    }

    // Phase A 分桶：同 fast_hash 组内 winner 顺序按 `full_path` 字典序确定。
    // 循环 3 次让并行调度差异下同 hash 组内 dedup 结果稳定（Report.copied == 1）。
    #[test]
    fn run_copy_loop_dedup_produces_single_winner_deterministically() {
        for _run in 0..3 {
            let src = tempdir().unwrap();
            let out = tempdir().unwrap();

            // 三份同内容 PNG，name 是 a/m/z → winner 稳定为 a（字典序首）
            let base = write_unique_png(src.path(), "z_last.png", 42);
            dup_png(&base, src.path(), "a_first.png");
            dup_png(&base, src.path(), "m_mid.png");

            let report = copy(
                &[local_source(src.path())],
                local_source(out.path()),
                false,
                false,
                false,
                Some(DEFAULT_TMPL),
                None,
            )
            .unwrap();
            assert_eq!(
                (report.copied, report.ignored, report.failed),
                (1, 2, 0),
                "3 同 hash 源必稳定 copy 1 + ignore 2"
            );
        }
    }
}
