export type Metrics = Record<string, number>;

const TIME_L_FIELDS: [string, string, number][] = [
  ["maximum resident set size", "max_rss_kb", 1 / 1024],
  ["peak memory footprint", "peak_private_kb", 1 / 1024],
  ["page reclaims", "minor_page_faults", 1],
  ["page faults", "major_page_faults", 1],
  ["block input operations", "fs_inputs", 1],
  ["block output operations", "fs_outputs", 1],
  ["voluntary context switches", "vol_ctx_switches", 1],
  ["involuntary context switches", "invol_ctx_switches", 1],
  ["instructions retired", "instructions_retired", 1],
  ["cycles elapsed", "cycles", 1],
];

export function parseTimeL(text: string): Metrics {
  const result: Metrics = {};
  for (const line of text.split(/\r?\n/)) {
    const times = /^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$/.exec(line);
    if (times) {
      result.elapsed_sec = parseFloat(times[1]);
      result.user_time_sec = parseFloat(times[2]);
      result.system_time_sec = parseFloat(times[3]);
      continue;
    }
    const counter = /^\s*(\d+)\s+(.+?)\s*$/.exec(line);
    if (!counter) continue;
    const field = TIME_L_FIELDS.find(([label]) => label === counter[2]);
    if (field) result[field[1]] = Math.round(parseInt(counter[1], 10) * field[2]);
  }
  return result;
}

const SAMPLE_FIELDS: [string, string][] = [
  ["Elapsed seconds:", "elapsed_sec"],
  ["Maximum resident set size (kbytes):", "max_rss_kb"],
  ["Peak private bytes (kbytes):", "peak_private_kb"],
  ["User time (seconds):", "user_time_sec"],
  ["System time (seconds):", "system_time_sec"],
  ["Exit status:", "exit_status"],
];

export function parseSample(text: string): Metrics {
  const result: Metrics = {};
  for (const line of text.split(/\r?\n/)) {
    const stripped = line.trim();
    const field = SAMPLE_FIELDS.find(([prefix]) => stripped.startsWith(prefix));
    if (!field) continue;
    const value = parseFloat(stripped.slice(field[0].length).trim());
    if (!Number.isNaN(value)) result[field[1]] = value;
  }
  return result;
}

function psQuote(s: string): string {
  return `'${s.replaceAll("'", "''")}'`;
}

export function psSampleScript(bin: string, args: string[], stdoutPath: string, stderrPath: string): string {
  const invariant = "ToString([Globalization.CultureInfo]::InvariantCulture)";
  return [
    "$t = [Diagnostics.Stopwatch]::StartNew()",
    `$p = Start-Process -FilePath ${psQuote(bin)} -ArgumentList ${args.map(psQuote).join(",")} -NoNewWindow -PassThru -RedirectStandardOutput ${psQuote(stdoutPath)} -RedirectStandardError ${psQuote(stderrPath)}`,
    "$null = $p.Handle; $ws = [long]0; $pv = [long]0",
    "while (-not $p.HasExited) { $p.Refresh(); $ws = [Math]::Max($ws, $p.PeakWorkingSet64); $pv = [Math]::Max($pv, $p.PeakPagedMemorySize64); Start-Sleep -Milliseconds 50 }",
    "$p.WaitForExit()",
    `'Elapsed seconds: ' + $t.Elapsed.TotalSeconds.${invariant}`,
    "'Maximum resident set size (kbytes): ' + [long]($ws / 1KB)",
    "'Peak private bytes (kbytes): ' + [long]($pv / 1KB)",
    `'User time (seconds): ' + $p.UserProcessorTime.TotalSeconds.${invariant}`,
    `'System time (seconds): ' + $p.PrivilegedProcessorTime.TotalSeconds.${invariant}`,
    "'Exit status: ' + $p.ExitCode",
  ].join("; ");
}

export function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

export type Spread = { median: number; min: number; max: number };

export function summarize(runs: Metrics[]): Record<string, Spread> {
  const summary: Record<string, Spread> = {};
  const names = new Set(runs.flatMap((run) => Object.keys(run)));
  for (const name of names) {
    const values = runs.map((run) => run[name]).filter((v) => v !== undefined);
    summary[name] = { median: median(values), min: Math.min(...values), max: Math.max(...values) };
  }
  return summary;
}

export const EXIT_OVER_LIMIT = 3;
export const EXIT_NO_METRIC = 4;

export function memoryExit(runs: Metrics[], maxPrivateMib: number | null): number {
  if (maxPrivateMib === null) return 0;
  if (runs.some((run) => run.peak_private_kb === undefined)) return EXIT_NO_METRIC;
  return runs.some((run) => run.peak_private_kb > maxPrivateMib * 1024) ? EXIT_OVER_LIMIT : 0;
}
