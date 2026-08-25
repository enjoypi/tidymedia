// tidymedia 真实数据性能采集：一次跑产 AI 可读性能报告。
//
// 用法：
//   bun scripts/perf-collect.ts --sub copy --data /path/to/photos --output-dir /tmp/perf-run
//
// 产物（--output-dir 下）：
//   report.json     tidymedia --report 落地的 Report JSON（含 duration_ms）
//   time-v.txt      /usr/bin/time -v 抓的 RSS/CPU/IO 统计原文
//   perf-report.md  单一汇总 markdown，直接扔给 LLM 分析

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const SUBS = ["copy", "move", "find", "cull", "move-text-shot"];

type Cast = "int" | "float" | "str";

const TIME_V_FIELDS: [string, string, Cast][] = [
  ["Maximum resident set size (kbytes):", "max_rss_kb", "int"],
  ["Elapsed (wall clock) time (h:mm:ss or m:ss):", "elapsed_wall", "str"],
  ["User time (seconds):", "user_time_sec", "float"],
  ["System time (seconds):", "system_time_sec", "float"],
  ["Percent of CPU this job got:", "cpu_percent", "str"],
  ["File system inputs:", "fs_inputs", "int"],
  ["File system outputs:", "fs_outputs", "int"],
  ["Voluntary context switches:", "vol_ctx_switches", "int"],
  ["Involuntary context switches:", "invol_ctx_switches", "int"],
  ["Page size (bytes):", "page_size_bytes", "int"],
  ["Minor (reclaiming a frame) page faults:", "minor_page_faults", "int"],
  ["Major (requiring I/O) page faults:", "major_page_faults", "int"],
];

function parseTimeV(text: string): Record<string, string | number> {
  const result: Record<string, string | number> = {};
  for (const line of text.split(/\r?\n/)) {
    const stripped = line.trim();
    for (const [prefix, name, cast] of TIME_V_FIELDS) {
      if (!stripped.startsWith(prefix)) continue;
      const raw = stripped.slice(prefix.length).trim();
      if (cast === "int") {
        const n = parseInt(raw, 10);
        result[name] = Number.isNaN(n) ? raw : n;
      } else if (cast === "float") {
        const f = parseFloat(raw);
        result[name] = Number.isNaN(f) ? raw : f;
      } else {
        result[name] = raw;
      }
      break;
    }
  }
  return result;
}

function parseIsoNow(): string {
  return new Date().toISOString().slice(0, 19) + "Z";
}

function findBinary(projectRoot: string): string {
  const candidate = resolve(projectRoot, "target", "release", "tidymedia");
  if (existsSync(candidate)) return candidate;
  console.error(`error: ${candidate} not found; build first with cargo build --release`);
  process.exit(1);
}

function findGnuTime(): string | null {
  for (const path of ["/usr/bin/time", "/usr/local/bin/gtime", "/opt/homebrew/bin/gtime"]) {
    if (existsSync(path)) return path;
  }
  return null;
}

function buildCli(
  sub: string,
  data: string,
  extra: string[],
  reportPath: string,
  outputTarget: string | null,
): string[] {
  const args = [sub];
  if (sub === "find") {
    args.push(data);
    if (outputTarget) args.push("-o", outputTarget);
  } else if (sub === "cull") {
    args.push(data);
    if (outputTarget) args.push("-o", outputTarget);
  } else if (sub === "move-text-shot") {
    args.push("--dry-run", data);
    if (outputTarget) args.push("-o", outputTarget);
  } else {
    args.push("--dry-run", data);
    if (outputTarget) args.push("-o", outputTarget);
  }
  args.push("--report", reportPath);
  args.push(...extra);
  return args;
}

function runWithTimeV(
  timeBin: string,
  tidyBin: string,
  cliArgs: string[],
  timeVOut: string,
  env: Record<string, string | undefined>,
): number {
  const cmd = [timeBin, "-v", tidyBin, ...cliArgs];
  const proc = spawnSync(cmd[0], cmd.slice(1), { env, stdio: ["ignore", "ignore", "pipe"] });
  const stderr = proc.stderr ?? Buffer.alloc(0);
  writeFileSync(timeVOut, stderr);
  return proc.status ?? 1;
}

function fmt(val: unknown, spec = ""): string {
  if (val === null || val === undefined) return "n/a";
  if (typeof val === "number") {
    if (spec === ".3f") return val.toFixed(3);
    if (spec === ".2f") return val.toFixed(2);
    if (spec === ".1f") return val.toFixed(1);
  }
  return String(val);
}

function renderReport(
  sub: string,
  data: string,
  reportJson: Record<string, unknown>,
  timeV: Record<string, string | number>,
  returnCode: number,
  outputDir: string,
): string {
  const durationMs = Number(reportJson.duration_ms ?? 0);
  const durationSec = durationMs ? durationMs / 1000 : 0;
  const bytesRead = reportJson.bytes_read as number | undefined;
  const scanned = Number(reportJson.scanned ?? 0);
  const throughputMiB =
    bytesRead && durationSec ? bytesRead / 1024 / 1024 / durationSec : null;
  const rssKb = Number(timeV.max_rss_kb ?? 0);
  const rssMiB = rssKb ? rssKb / 1024 : null;

  const lines = [
    "# tidymedia 性能采集报告",
    "",
    `- 时间：\`${parseIsoNow()}\``,
    `- 子命令：\`${sub}\``,
    `- 数据集：\`${data}\``,
    `- 输出目录：\`${outputDir}\``,
    `- 退出码：\`${returnCode}\``,
    "",
    "## L1 - Report 概览",
    "",
    "| 指标 | 值 |",
    "|---|---|",
    `| 扫描文件数 | ${scanned} |`,
    `| use case 耗时 | ${fmt(durationMs)} ms (${fmt(durationSec, ".3f")} s) |`,
    `| 累计读字节 | ${fmt(bytesRead)} |`,
    `| 吞吐 | ${throughputMiB ? fmt(throughputMiB, ".2f") : "n/a"} MiB/s |`,
    "",
    "## L4 - 系统资源（/usr/bin/time -v）",
    "",
    "| 指标 | 值 |",
    "|---|---|",
    `| Wall clock | ${fmt(timeV.elapsed_wall)} |`,
    `| 峰值 RSS | ${rssMiB ? fmt(rssMiB, ".1f") : "n/a"} MiB (${fmt(timeV.max_rss_kb)} KB) |`,
    `| User CPU | ${fmt(timeV.user_time_sec)} s |`,
    `| System CPU | ${fmt(timeV.system_time_sec)} s |`,
    `| CPU 利用率 | ${fmt(timeV.cpu_percent)} |`,
    `| 文件系统读 | ${fmt(timeV.fs_inputs)} blocks |`,
    `| 文件系统写 | ${fmt(timeV.fs_outputs)} blocks |`,
    `| Major page faults | ${fmt(timeV.major_page_faults)} |`,
    `| Minor page faults | ${fmt(timeV.minor_page_faults)} |`,
    `| 主动上下文切换 | ${fmt(timeV.vol_ctx_switches)} |`,
    `| 被动上下文切换 | ${fmt(timeV.invol_ctx_switches)} |`,
    "",
    "## AI 分析建议提示",
    "",
    "把本报告 + `report.json` + `time-v.txt` 一起扔给 LLM 提问：",
    "",
    "1. 吞吐 vs 峰值 RSS：是否 IO/CPU/内存受限？",
    "2. `User/System CPU` 比例：kernel 时间占比高 → 系统调用密集（open/read 小文件）",
    "3. `Major page faults` 高 → 内存不足开始换页；建议减小 `STREAM_CHUNK` 或增大 RAM",
    "4. `duration_ms` 与 `elapsed_wall` 差 → 后者含 Rust 启动 + tract 加载 ONNX 等固定开销",
    "",
  ];
  return lines.join("\n");
}

function usage(): void {
  console.error(
    "用法：bun scripts/perf-collect.ts --sub <copy|move|find|cull|move-text-shot> --data <dir> --output-dir <dir> [--output-target <path>] [--extra <args>] [--project-root <root>]",
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

const opts = parseArgv(process.argv.slice(2));
const sub = opts.sub;
const data = opts.data;
const outputDirArg = opts["output-dir"];
if (!sub || !outputDirArg || !SUBS.includes(sub) || !data) {
  usage();
  process.exit(2);
}
const outputTarget = opts["output-target"] ?? null;
const projectRoot = resolve(opts["project-root"] ?? ".");
const outputDir = resolve(outputDirArg);
mkdirSync(outputDir, { recursive: true });

const tidyBin = findBinary(projectRoot);
const timeBin = findGnuTime();
if (timeBin === null) {
  console.error(
    "error: /usr/bin/time not found (Linux) or gtime not installed (macOS)",
  );
  process.exit(1);
}

const reportPath = resolve(outputDir, "report.json");
const timeVPath = resolve(outputDir, "time-v.txt");
const extraArgs = opts.extra ? opts.extra.split(/\s+/) : [];
const cliArgs = buildCli(sub, data, extraArgs, reportPath, outputTarget);

const env: Record<string, string | undefined> = { ...process.env };
env.CARGO_PROFILE_RELEASE_OPT_LEVEL ??= "3";

console.error(`[perf-collect] running: ${timeBin} -v ${tidyBin} ${cliArgs.join(" ")}`);
const returnCode = runWithTimeV(timeBin, tidyBin, cliArgs, timeVPath, env);
console.error(`[perf-collect] tidymedia exit code: ${returnCode}`);

const timeVData = parseTimeV(readFileSync(timeVPath, "utf8"));
let reportData: Record<string, unknown> = {};
if (existsSync(reportPath)) {
  try {
    reportData = JSON.parse(readFileSync(reportPath, "utf8")) as Record<string, unknown>;
  } catch (e) {
    console.error(`warn: report.json parse failed: ${e}`);
  }
}

const md = renderReport(sub, data, reportData, timeVData, returnCode, outputDir);
writeFileSync(resolve(outputDir, "perf-report.md"), md);
console.error(`[perf-collect] wrote ${resolve(outputDir, "perf-report.md")}`);
process.exit(returnCode === 0 ? 0 : returnCode);
