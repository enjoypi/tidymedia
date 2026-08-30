//! verify CLI stdout 人类可读汇总（内化 tidy-verify skill `analyze_verify.ts`）：
//! summary 计数 / MISMATCH 明细（exiftool 期望桶 ≠ 预测桶）/ DIFFER 明细
//! （文件名桶 ≠ 预测桶）/ `duplicate_verdict` 分布 / pattern 计数。`dispatch_verify`
//! 打印，skill 不再需独立分析脚本。排序确定性：计数降序，同计数 key 升序。

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::report::VerifyReport;

#[must_use]
pub(crate) fn format_summary(report: &VerifyReport) -> String {
    let mut out = String::new();
    out.push_str("---summary---\n");
    let _ = writeln!(out, "scanned={}", report.scanned);
    let _ = writeln!(out, "compared={}", report.compared);
    let _ = writeln!(out, "mismatched={}", report.mismatched);
    let _ = writeln!(out, "decision_failed={}", report.decision_failed);
    let with_name_time = report
        .entries
        .iter()
        .filter(|e| e.filename_bucket.is_some())
        .count();
    let _ = writeln!(out, "with_name_time={with_name_time}");

    let mismatches: Vec<_> = report.entries.iter().filter(|e| e.mismatch).collect();
    let _ = writeln!(out, "MISMATCH_count={}", mismatches.len());
    if !mismatches.is_empty() {
        out.push_str("---mismatch by from---\n");
        let mut by_from: BTreeMap<&str, usize> = BTreeMap::new();
        for e in &mismatches {
            *by_from
                .entry(e.exif_from.as_deref().unwrap_or("NONE"))
                .or_insert(0) += 1;
        }
        for (from, n) in &by_from {
            let _ = writeln!(out, "{n}\t{from}");
        }
        out.push_str("---MISMATCH details---\n");
        for e in &mismatches {
            let exp = e.exif_exp_bucket.as_deref().unwrap_or("NONE");
            let from = e.exif_from.as_deref().unwrap_or("NONE");
            let make = non_empty(e.exif_make.as_deref());
            let model = non_empty(e.exif_model.as_deref());
            let _ = writeln!(
                out,
                "MISMATCH\texp={exp}\ttgt={}\tfrom={from}\tmake={make}\tmodel={model}\t{}",
                e.actual_bucket, e.source_path
            );
        }
    }

    let differs: Vec<_> = report
        .entries
        .iter()
        .filter(|e| {
            e.filename_bucket
                .as_ref()
                .is_some_and(|n| *n != e.actual_bucket)
        })
        .collect();
    let _ = writeln!(out, "DIFFER_count={}", differs.len());
    if !differs.is_empty() {
        out.push_str("---DIFFER details---\n");
        for e in &differs {
            let name = e.filename_bucket.as_deref().unwrap_or("NONE");
            let _ = writeln!(
                out,
                "DIFFER\tname={name}\ttgt={}\t{}",
                e.actual_bucket, e.source_path
            );
        }
    }

    out.push_str("---duplicate_verdict---\n");
    let mut verdict_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &report.entries {
        *verdict_counts
            .entry(e.duplicate_verdict.as_str())
            .or_insert(0) += 1;
    }
    for (verdict, n) in sorted_counts(verdict_counts.iter()) {
        let _ = writeln!(out, "{n}\t{verdict}");
    }

    if !report.pattern_counts.is_empty() {
        out.push_str("---patterns---\n");
        for (pattern, n) in sorted_counts(report.pattern_counts.iter()) {
            let _ = writeln!(out, "{n}\t{pattern}");
        }
    }
    out
}

fn non_empty(v: Option<&str>) -> &str {
    v.filter(|s| !s.is_empty()).unwrap_or("-")
}

fn sorted_counts<'a, K: AsRef<str> + 'a>(
    iter: impl Iterator<Item = (&'a K, &'a usize)>,
) -> Vec<(&'a str, usize)> {
    let mut v: Vec<_> = iter.map(|(k, n)| (k.as_ref(), *n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    v
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
