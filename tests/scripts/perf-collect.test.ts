import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, sep } from "node:path";
import { buildCli, dataStats, pathUnder, realish, validateArgs } from "../../scripts/perf-collect.ts";
import {
  EXIT_NO_METRIC,
  EXIT_OVER_LIMIT,
  median,
  memoryExit,
  parseSample,
  parseTimeL,
  psSampleScript,
  summarize,
} from "../../scripts/perf-measure.ts";

test("buildCli verify 不加 --dry-run 且带 -o", () => {
  expect(buildCli("verify", "S", [], "R.json", "O")).toEqual(["verify", "S", "-o", "O", "--report", "R.json"]);
});

test("buildCli move 与 copy-doc 默认 --dry-run", () => {
  expect(buildCli("move", "S", ["-x"], "R.json", "O")).toEqual([
    "move", "--dry-run", "S", "-o", "O", "--report", "R.json", "-x",
  ]);
  expect(buildCli("copy-doc", "S", [], "R.json", "O").slice(0, 2)).toEqual(["copy-doc", "--dry-run"]);
});

test("parseTimeL 解析 macOS time -l 并把字节换算为 KB", () => {
  const text = [
    "tidymedia stderr noise 12 real",
    "        1.25 real         0.75 user         0.50 sys",
    "             2097152  maximum resident set size",
    "                   7  page reclaims",
    "                   2  page faults",
    "                  11  block input operations",
    "                   3  block output operations",
    "                   5  voluntary context switches",
    "                   9  involuntary context switches",
    "            11365194  instructions retired",
    "             5289667  cycles elapsed",
    "             1048576  peak memory footprint",
  ].join("\n");
  expect(parseTimeL(text)).toEqual({
    elapsed_sec: 1.25,
    user_time_sec: 0.75,
    system_time_sec: 0.5,
    max_rss_kb: 2048,
    minor_page_faults: 7,
    major_page_faults: 2,
    fs_inputs: 11,
    fs_outputs: 3,
    vol_ctx_switches: 5,
    invol_ctx_switches: 9,
    instructions_retired: 11365194,
    cycles: 5289667,
    peak_private_kb: 1024,
  });
});

test("parseSample 解析 Windows 采样输出并忽略非数值", () => {
  const text = [
    "Elapsed seconds: 5.1",
    "Maximum resident set size (kbytes): 5658624",
    "Peak private bytes (kbytes): 342016",
    "User time (seconds): 12.5",
    "System time (seconds): oops",
    "Exit status: 0",
  ].join("\r\n");
  expect(parseSample(text)).toEqual({
    elapsed_sec: 5.1,
    max_rss_kb: 5658624,
    peak_private_kb: 342016,
    user_time_sec: 12.5,
    exit_status: 0,
  });
});

test("psSampleScript 单引号转义、保存 stdout 并取内核峰值私有内存", () => {
  const s = psSampleScript("D:/t/tidymedia.exe", ["verify", "D:/it's"], "D:/o/out.txt", "D:/o/err.log");
  expect(s).toContain("'D:/it''s'");
  expect(s).toContain("-RedirectStandardOutput 'D:/o/out.txt'");
  expect(s).toContain("-RedirectStandardError 'D:/o/err.log'");
  expect(s).toContain("$ws = [long]0; $pv = [long]0");
  expect(s).toContain("PeakPagedMemorySize64");
  for (const key of ["Elapsed seconds", "Maximum resident set size", "Peak private bytes", "User time", "System time", "Exit status"]) {
    expect(s).toContain(key);
  }
});

test("median 与 summarize 给出中位数和极值", () => {
  expect(median([3, 1, 2])).toBe(2);
  expect(median([4, 1, 2, 3])).toBe(2.5);
  expect(summarize([{ a: 1, b: 5 }, { a: 3 }])).toEqual({
    a: { median: 2, min: 1, max: 3 },
    b: { median: 5, min: 5, max: 5 },
  });
});

test("memoryExit 未设阈值通过，超限 3，缺指标 4", () => {
  expect(memoryExit([{}], null)).toBe(0);
  expect(memoryExit([{ peak_private_kb: 2048 * 1024 }], 2048)).toBe(0);
  expect(memoryExit([{ peak_private_kb: 1 }, { peak_private_kb: 2048 * 1024 + 1 }], 2048)).toBe(EXIT_OVER_LIMIT);
  expect(memoryExit([{ peak_private_kb: 1 }, {}], 2048)).toBe(EXIT_NO_METRIC);
});

test("validateArgs 除 find 外强制 --output-target", () => {
  for (const sub of ["copy", "move", "copy-doc", "move-doc", "move-text-shot", "verify"]) {
    expect(validateArgs({ sub, data: "S", "output-dir": "O" })).toBe("--output-target");
    expect(validateArgs({ sub, data: "S", "output-dir": "O", "output-target": "T" })).toBeNull();
  }
  expect(validateArgs({ sub: "find", data: "S", "output-dir": "O" })).toBeNull();
});

test("validateArgs 缺 sub/data/output-dir 分别报错", () => {
  expect(validateArgs({})).toBe("--sub");
  expect(validateArgs({ sub: "nope", data: "S", "output-dir": "O" })).toBe("--sub");
  expect(validateArgs({ sub: "find" })).toBe("--data");
  expect(validateArgs({ sub: "find", data: "S" })).toBe("--output-dir");
});

test("validateArgs 校验 repeat、warmup 与内存阈值", () => {
  const base = { sub: "find", data: "S", "output-dir": "O" };
  expect(validateArgs({ ...base, repeat: "0" })).toBe("--repeat");
  expect(validateArgs({ ...base, warmup: "x" })).toBe("--warmup");
  expect(validateArgs({ ...base, "max-private-mib": "0" })).toBe("--max-private-mib");
  expect(validateArgs({ ...base, repeat: "5", warmup: "0", "max-private-mib": "2048" })).toBeNull();
});

test("pathUnder 按分隔符边界判断包含", () => {
  expect(pathUnder(`${sep}a${sep}b`, `${sep}a`)).toBe(true);
  expect(pathUnder(`${sep}a`, `${sep}a`)).toBe(true);
  expect(pathUnder(`${sep}ab`, `${sep}a`)).toBe(false);
  expect(pathUnder(`${sep}a${sep}b`, `${sep}a${sep}`)).toBe(true);
});

test("realish 对不存在的路径解析最近的已存在祖先", () => {
  const root = mkdtempSync(join(tmpdir(), "tm-perf-"));
  expect(realish(join(root, "x", "y"))).toBe(join(realpathSync(root), "x", "y"));
  expect(realish(root)).toBe(realpathSync(root));
});

test("dataStats 跳过隐藏项并按扩展名计数", () => {
  const root = mkdtempSync(join(tmpdir(), "tm-perf-"));
  mkdirSync(join(root, "sub"));
  mkdirSync(join(root, ".git"));
  writeFileSync(join(root, "a.MP4"), "12345");
  writeFileSync(join(root, "sub", "b.mp4"), "123");
  writeFileSync(join(root, ".hidden"), "x");
  writeFileSync(join(root, ".git", "c.mp4"), "x");
  expect(dataStats(root)).toEqual({ files: 2, bytes: 8, exts: { ".mp4": 2 } });
});
