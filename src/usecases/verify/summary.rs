use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Write};

use super::report::{VerifyEntry, VerifyReport};

#[derive(Default)]
struct Counts {
    with_name_time: usize,
    mismatches: usize,
    differs: usize,
    by_from: BTreeMap<String, usize>,
    verdicts: BTreeMap<String, usize>,
}

impl Counts {
    fn add(&mut self, e: &VerifyEntry) {
        self.with_name_time += usize::from(e.filename_bucket.is_some());
        self.differs += usize::from(differs_from_name(e));
        *self
            .verdicts
            .entry(e.duplicate_verdict.clone())
            .or_insert(0) += 1;
        if e.mismatch {
            self.mismatches += 1;
            *self
                .by_from
                .entry(e.exif_from.clone().unwrap_or_else(|| "NONE".to_owned()))
                .or_insert(0) += 1;
        }
    }

    fn head(&self, report: &VerifyReport) -> String {
        let mut out = String::from("---summary---\n");
        let _ = writeln!(out, "scanned={}", report.scanned);
        let _ = writeln!(out, "compared={}", report.compared);
        let _ = writeln!(out, "mismatched={}", report.mismatched);
        let _ = writeln!(out, "decision_failed={}", report.decision_failed);
        let _ = writeln!(out, "exif_row_missing={}", report.exif_row_missing);
        let _ = writeln!(out, "with_name_time={}", self.with_name_time);
        let _ = writeln!(out, "MISMATCH_count={}", self.mismatches);
        if self.mismatches > 0 {
            out.push_str("---mismatch by from---\n");
            for (from, n) in &self.by_from {
                let _ = writeln!(out, "{n}\t{from}");
            }
            out.push_str("---MISMATCH details---\n");
        }
        out
    }

    fn middle(&self) -> String {
        let mut out = format!("DIFFER_count={}\n", self.differs);
        if self.differs > 0 {
            out.push_str("---DIFFER details---\n");
        }
        out
    }

    fn tail(&self, report: &VerifyReport) -> String {
        let mut out = String::from("---duplicate_verdict---\n");
        for (verdict, n) in sorted_counts(self.verdicts.iter()) {
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
}

pub(crate) fn write_summary(report: &VerifyReport, out: &mut dyn Write) -> io::Result<()> {
    let mut counts = Counts::default();
    report
        .entries
        .for_each(&mut |e| {
            counts.add(e);
            Ok(())
        })
        .and_then(|()| out.write_all(counts.head(report).as_bytes()))
        .and_then(|()| write_rows(report, out, &|e| e.mismatch.then(|| mismatch_row(e))))
        .and_then(|()| out.write_all(counts.middle().as_bytes()))
        .and_then(|()| {
            write_rows(report, out, &|e| {
                differs_from_name(e).then(|| differ_row(e))
            })
        })
        .and_then(|()| out.write_all(counts.tail(report).as_bytes()))
}

#[cfg(test)]
pub(crate) fn format_summary(report: &VerifyReport) -> String {
    let mut out = Vec::new();
    write_summary(report, &mut out).expect("internal: writing to Vec cannot fail");
    String::from_utf8(out).expect("internal: summary is UTF-8")
}

fn write_rows(
    report: &VerifyReport,
    out: &mut dyn Write,
    row: &dyn Fn(&VerifyEntry) -> Option<String>,
) -> io::Result<()> {
    report
        .entries
        .for_each(&mut |e| row(e).map_or(Ok(()), |line| out.write_all(line.as_bytes())))
}

fn differs_from_name(e: &VerifyEntry) -> bool {
    e.filename_bucket
        .as_ref()
        .is_some_and(|n| *n != e.actual_bucket)
}

fn mismatch_row(e: &VerifyEntry) -> String {
    let exp = e.exif_exp_bucket.as_deref().unwrap_or("NONE");
    let from = e.exif_from.as_deref().unwrap_or("NONE");
    let make = non_empty(e.exif_make.as_deref());
    let model = non_empty(e.exif_model.as_deref());
    format!(
        "MISMATCH\texp={exp}\ttgt={}\tfrom={from}\tmake={make}\tmodel={model}\t{}\n",
        e.actual_bucket, e.source_path
    )
}

fn differ_row(e: &VerifyEntry) -> String {
    let name = e.filename_bucket.as_deref().unwrap_or("NONE");
    format!(
        "DIFFER\tname={name}\ttgt={}\t{}\n",
        e.actual_bucket, e.source_path
    )
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
