import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { cpus, loadavg } from "node:os";
import { basename, dirname, extname, join, resolve, sep } from "node:path";
import { type Metrics, memoryExit, parseSample, parseTimeL, psSampleScript, summarize } from "./perf-measure.ts";

const SUBS = ["copy", "move", "copy-doc", "move-doc", "find", "move-text-shot", "verify"];
const NO_DRY_RUN_SUBS = ["find", "verify"];
const IS_WINDOWS = process.platform === "win32";
const TIME_BIN = "/usr/bin/time";

export function buildCli(
  sub: string,
  data: string,
  extra: string[],
  reportPath: string,
  outputTarget: string | null,
): string[] {
  const args = NO_DRY_RUN_SUBS.includes(sub) ? [sub, data] : [sub, "--dry-run", data];
  if (outputTarget) args.push("-o", outputTarget);
  args.push("--report", reportPath);
  args.push(...extra);
  return args;
}

function positiveInt(raw: string | undefined, min: number): boolean {
  return raw === undefined || (/^\d+$/.test(raw) && parseInt(raw, 10) >= min);
}

export function validateArgs(opts: Record<string, string>): string | null {
  const sub = opts.sub;
  if (!sub || !SUBS.includes(sub)) return "--sub";
  if (!opts.data) return "--data";
  if (!opts["output-dir"]) return "--output-dir";
  if (sub !== "find" && !opts["output-target"]) return "--output-target";
  if (!positiveInt(opts.repeat, 1)) return "--repeat";
  if (!positiveInt(opts.warmup, 0)) return "--warmup";
  if (!positiveInt(opts["max-private-mib"], 1)) return "--max-private-mib";
  return null;
}

export function pathUnder(child: string, parent: string): boolean {
  return child === parent || child.startsWith(parent.endsWith(sep) ? parent : parent + sep);
}

export function realish(path: string, exists: (p: string) => boolean = existsSync): string {
  const full = resolve(path);
  if (exists(full)) return realpathSync(full);
  const parent = dirname(full);
  return parent === full ? full : join(realish(parent, exists), basename(full));
}

export type DataStats = { files: number; bytes: number; exts: Record<string, number> };

export function dataStats(dir: string): DataStats {
  const stats: DataStats = { files: 0, bytes: 0, exts: {} };
  const pending = [dir];
  while (pending.length) {
    const current = pending.pop() as string;
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue;
      const path = join(current, entry.name);
      if (entry.isDirectory()) pending.push(path);
      if (!entry.isFile()) continue;
      const ext = extname(entry.name).toLowerCase();
      stats.files += 1;
      stats.bytes += statSync(path).size;
      stats.exts[ext] = (stats.exts[ext] ?? 0) + 1;
    }
  }
  return stats;
}

function newestMtime(path: string): number {
  const stat = statSync(path);
  if (!stat.isDirectory()) return stat.mtimeMs;
  return Math.max(0, ...readdirSync(path).map((name) => newestMtime(join(path, name))));
}

function findBinary(projectRoot: string): string {
  const candidate = resolve(projectRoot, "target", "release", IS_WINDOWS ? "tidymedia.exe" : "tidymedia");
  if (!existsSync(candidate)) {
    console.error(`error: ${candidate} not found; build first with just build-release`);
    process.exit(1);
  }
  const sources = Math.max(newestMtime(resolve(projectRoot, "src")), newestMtime(resolve(projectRoot, "Cargo.lock")));
  if (statSync(candidate).mtimeMs < sources) {
    console.error(`error: ${candidate} is older than sources; rebuild with just build-release`);
    process.exit(1);
  }
  return candidate;
}

function git(projectRoot: string, args: string[]): string {
  return (spawnSync("git", args, { cwd: projectRoot }).stdout ?? Buffer.alloc(0)).toString("utf8").trim();
}

function runOnce(tidyBin: string, cliArgs: string[], runDir: string): { code: number; metrics: Metrics } {
  mkdirSync(runDir, { recursive: true });
  const stdoutPath = resolve(runDir, "stdout.txt");
  const timePath = resolve(runDir, "time.txt");
  if (IS_WINDOWS) {
    const script = psSampleScript(tidyBin, cliArgs, stdoutPath, resolve(runDir, "stderr.txt"));
    const proc = spawnSync("powershell.exe", ["-NoProfile", "-Command", script], { stdio: ["ignore", "pipe", "inherit"] });
    writeFileSync(timePath, proc.stdout ?? Buffer.alloc(0));
    const metrics = parseSample(readFileSync(timePath, "utf8"));
    return { code: metrics.exit_status ?? 1, metrics };
  }
  const proc = spawnSync(TIME_BIN, ["-l", tidyBin, ...cliArgs], { stdio: ["ignore", "pipe", "pipe"], maxBuffer: 1 << 30 });
  writeFileSync(stdoutPath, proc.stdout ?? Buffer.alloc(0));
  writeFileSync(timePath, proc.stderr ?? Buffer.alloc(0));
  return { code: proc.status ?? 1, metrics: parseTimeL(readFileSync(timePath, "utf8")) };
}

function readReport(path: string): Record<string, unknown> {
  if (!existsSync(path)) return {};
  return JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown>;
}

function usage(): void {
  console.error(
    `用法：bun scripts/perf-collect.ts --sub <${SUBS.join("|")}> --data <dir> --output-dir <dir> --output-target <path（find 除外必填）> [--repeat <n>] [--warmup <n>] [--max-private-mib <n>] [--extra <args>] [--binary <path>] [--project-root <root>]`,
  );
}

function parseArgv(argv: string[]): Record<string, string> {
  const opts: Record<string, string> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith("--")) continue;
    const next = argv[i + 1];
    if (next === undefined || next.startsWith("--")) {
      console.error(`error: missing value for ${a}`);
      process.exit(2);
    }
    opts[a.slice(2)] = next;
    i++;
  }
  return opts;
}

function main(): void {
  const opts = parseArgv(process.argv.slice(2));
  const invalid = validateArgs(opts);
  if (invalid) {
    console.error(`error: missing or invalid ${invalid}`);
    usage();
    process.exit(2);
  }
  const sub = opts.sub;
  const data = realish(opts.data);
  const outputDir = realish(opts["output-dir"]);
  const outputTarget = opts["output-target"] ? realish(opts["output-target"]) : null;
  for (const out of [outputDir, outputTarget]) {
    if (out !== null && pathUnder(out, data)) {
      console.error(`error: ${out} is inside --data ${data}`);
      process.exit(2);
    }
  }
  const projectRoot = resolve(opts["project-root"] ?? ".");
  const tidyBin = opts.binary ? resolve(opts.binary) : findBinary(projectRoot);
  const repeat = parseInt(opts.repeat ?? "1", 10);
  const warmup = parseInt(opts.warmup ?? "0", 10);
  const maxPrivateMib = opts["max-private-mib"] ? parseInt(opts["max-private-mib"], 10) : null;
  const extraArgs = opts.extra ? opts.extra.split(/\s+/) : [];

  const runs: Metrics[] = [];
  let report: Record<string, unknown> = {};
  let argv: string[] = [];
  let exitCode = 0;
  for (let i = -warmup; i < repeat; i++) {
    const runDir = resolve(outputDir, "runs", i < 0 ? `warmup-${-i}` : String(i));
    mkdirSync(runDir, { recursive: true });
    const reportPath = resolve(runDir, "report.json");
    argv = buildCli(sub, data, extraArgs, reportPath, outputTarget);
    const { code, metrics } = runOnce(tidyBin, argv, runDir);
    console.error(`[perf-collect] run ${i}: exit ${code}, peak private ${metrics.peak_private_kb ?? "n/a"} KB`);
    if (i < 0) continue;
    runs.push(metrics);
    report = readReport(reportPath);
    exitCode ||= code;
  }

  const memory = memoryExit(runs, maxPrivateMib);
  const perf = {
    argv: [tidyBin, ...argv],
    sub,
    dry_run: !NO_DRY_RUN_SUBS.includes(sub),
    data: { path: data, ...dataStats(data) },
    report,
    platform: process.platform,
    cpus: cpus().length,
    loadavg: loadavg(),
    git: { head: git(projectRoot, ["rev-parse", "HEAD"]), dirty: git(projectRoot, ["status", "--porcelain"]) !== "" },
    binary: { path: tidyBin, mtime_ms: statSync(tidyBin).mtimeMs },
    output_dir: outputDir,
    max_private_mib: maxPrivateMib,
    memory_exit: memory,
    runs,
    summary: summarize(runs),
  };
  writeFileSync(resolve(outputDir, "perf.json"), JSON.stringify(perf, null, 2));
  console.error(`[perf-collect] wrote ${resolve(outputDir, "perf.json")}`);
  process.exit(exitCode || memory);
}

if (import.meta.main) main();
