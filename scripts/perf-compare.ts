import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { Metrics } from "./perf-measure.ts";

const METRICS = ["elapsed_sec", "user_time_sec", "system_time_sec", "instructions_retired", "peak_private_kb"];
const VOLATILE_REPORT_FIELDS = ["duration_ms", "bytes_read"];

export type Verdict = "improved" | "regressed" | "noise";

export function verdict(base: number[], candidate: number[]): Verdict {
  if (!base.length || !candidate.length) return "noise";
  if (Math.max(...candidate) < Math.min(...base)) return "improved";
  if (Math.min(...candidate) > Math.max(...base)) return "regressed";
  return "noise";
}

export function compareMetrics(base: Metrics[], candidate: Metrics[]): Record<string, Verdict> {
  const pick = (runs: Metrics[], name: string) => runs.map((run) => run[name]).filter((v) => v !== undefined);
  return Object.fromEntries(METRICS.map((name) => [name, verdict(pick(base, name), pick(candidate, name))]));
}

export function stableReport(report: Record<string, unknown>): string {
  const kept = Object.entries(report).filter(([key]) => !VOLATILE_REPORT_FIELDS.includes(key));
  return JSON.stringify(Object.fromEntries(kept.sort(([a], [b]) => a.localeCompare(b))));
}

type Perf = { output_dir: string; runs: Metrics[]; report: Record<string, unknown> };

function load(dir: string): Perf {
  return JSON.parse(readFileSync(resolve(dir, "perf.json"), "utf8")) as Perf;
}

function lastStdout(perf: Perf): string {
  const path = resolve(perf.output_dir, "runs", String(perf.runs.length - 1), "stdout.txt");
  return existsSync(path) ? readFileSync(path, "utf8") : "";
}

function main(): void {
  const [baseDir, candidateDir] = process.argv.slice(2);
  if (!baseDir || !candidateDir) {
    console.error("用法：bun scripts/perf-compare.ts <基线产物目录> <候选产物目录>");
    process.exit(2);
  }
  const base = load(baseDir);
  const candidate = load(candidateDir);
  const result = {
    metrics: compareMetrics(base.runs, candidate.runs),
    stdout_same: lastStdout(base) === lastStdout(candidate),
    report_same: stableReport(base.report) === stableReport(candidate.report),
  };
  console.log(JSON.stringify(result, null, 2));
  process.exit(result.stdout_same && result.report_same ? 0 : 1);
}

if (import.meta.main) main();
