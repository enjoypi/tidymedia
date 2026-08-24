// 解析 `tidymedia verify --report` 产出的 VerifyReport JSON，汇总结论供 AI 阅读。
// 纯函数便于测试；字段名与 src/usecases/verify/report.rs 的 serde 契约一一对应。

export interface VerifyEntry {
  source_path: string;
  actual_bucket: string;
  chosen_priority: string;
  chosen_source: string;
  conflicts: string[];
  exif_exp_bucket: string | null;
  exif_from: string | null;
  exif_make: string | null;
  exif_model: string | null;
  filename_bucket: string | null;
  mismatch: boolean;
  duplicate_verdict: string;
  patterns: string[];
  fix_suggestion: string | null;
}

export interface VerifyReport {
  scanned: number;
  compared: number;
  mismatched: number;
  decision_failed: number;
  pattern_counts: Record<string, number>;
  entries: VerifyEntry[];
}

export interface MismatchRow {
  exp: string;
  tgt: string;
  from: string;
  make: string;
  model: string;
  source: string;
}

export interface DifferRow {
  name: string;
  tgt: string;
  source: string;
}

export interface VerifySummary {
  scanned: number;
  compared: number;
  mismatched: number;
  decision_failed: number;
  with_name_time: number;
  mismatchRows: MismatchRow[];
  differRows: DifferRow[];
  verdictCounts: Record<string, number>;
  patternCounts: Record<string, number>;
}

export function parseVerifyReport(json: unknown): VerifyReport {
  const r = (json ?? {}) as Record<string, unknown>;
  const entries = Array.isArray(r.entries) ? (r.entries as VerifyEntry[]) : [];
  return {
    scanned: num(r.scanned),
    compared: num(r.compared),
    mismatched: num(r.mismatched),
    decision_failed: num(r.decision_failed),
    pattern_counts: (r.pattern_counts as Record<string, number>) ?? {},
    entries,
  };
}

// 汇总：MISMATCH（exif_exp_bucket ≠ actual_bucket）、DIFFER（filename_bucket ≠
// actual_bucket）、duplicate_verdict 分布、pattern 计数。口径与旧 compare_buckets.py /
// filename_conflict.py / 45_check_copied.py 对齐。
export function analyzeVerifyReport(json: unknown): VerifySummary {
  const r = parseVerifyReport(json);
  const mismatchRows: MismatchRow[] = [];
  const differRows: DifferRow[] = [];
  const verdictCounts: Record<string, number> = {};
  // verify 的 pattern_counts 已含全部 entries 的 patterns 计数，直接采用不重复累加。
  const patternCounts: Record<string, number> = { ...r.pattern_counts };
  let withNameTime = 0;

  for (const e of r.entries) {
    const src = e.source_path ?? "";
    const tgt = e.actual_bucket ?? "";
    if (e.mismatch) {
      mismatchRows.push({
        exp: e.exif_exp_bucket ?? "NONE",
        tgt,
        from: e.exif_from ?? "NONE",
        make: e.exif_make || "-",
        model: e.exif_model || "-",
        source: src,
      });
    }
    if (e.filename_bucket) {
      withNameTime += 1;
      if (e.filename_bucket !== tgt) {
        differRows.push({ name: e.filename_bucket, tgt, source: src });
      }
    }
    const v = e.duplicate_verdict ?? "not_checked";
    verdictCounts[v] = (verdictCounts[v] ?? 0) + 1;
  }

  return {
    scanned: r.scanned,
    compared: r.compared,
    mismatched: r.mismatched,
    decision_failed: r.decision_failed,
    with_name_time: withNameTime,
    mismatchRows,
    differRows,
    verdictCounts,
    patternCounts,
  };
}

function num(v: unknown): number {
  return typeof v === "number" ? v : 0;
}
