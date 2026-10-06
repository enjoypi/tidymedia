import { expect, test } from "bun:test";
import { buildCli, parseTimeV, psSampleScript, renderReport } from "../../scripts/perf-collect.ts";

test("buildCli verify 不加 --dry-run 且带 -o", () => {
  expect(buildCli("verify", "S", [], "R.json", "O")).toEqual(["verify", "S", "-o", "O", "--report", "R.json"]);
});

test("buildCli move 默认 --dry-run", () => {
  expect(buildCli("move", "S", ["-x"], "R.json", "O")).toEqual([
    "move", "--dry-run", "S", "-o", "O", "--report", "R.json", "-x",
  ]);
});

test("parseTimeV 解析 Windows 采样输出", () => {
  const text = [
    "Elapsed (wall clock) time (h:mm:ss or m:ss): 0:00:05.10",
    "Maximum resident set size (kbytes): 5658624",
    "Peak private bytes (kbytes): 342016",
    "User time (seconds): 12.5",
    "System time (seconds): 3.25",
    "Exit status: 0",
  ].join("\r\n");
  expect(parseTimeV(text)).toEqual({
    elapsed_wall: "0:00:05.10",
    max_rss_kb: 5658624,
    peak_private_kb: 342016,
    user_time_sec: 12.5,
    system_time_sec: 3.25,
    exit_status: 0,
  });
});

test("psSampleScript 单引号转义并输出 GNU time 同名字段", () => {
  const s = psSampleScript("D:/t/tidymedia.exe", ["verify", "D:/it's"], "D:/o/err.log");
  expect(s).toContain("'D:/it''s'");
  expect(s).toContain("-RedirectStandardError 'D:/o/err.log'");
  expect(s).toContain("ToString('h\\:mm\\:ss\\.ff')");
  for (const key of ["Elapsed (wall clock)", "Maximum resident set size", "Peak private bytes", "User time", "System time", "Exit status"]) {
    expect(s).toContain(key);
  }
});

test("renderReport 输出峰值私有内存，缺失时 n/a", () => {
  const withPrivate = renderReport("verify", "S", {}, { peak_private_kb: 2048 }, 0, "O");
  expect(withPrivate).toContain("| 峰值私有内存 | 2.0 MiB |");
  expect(renderReport("verify", "S", {}, {}, 0, "O")).toContain("| 峰值私有内存 | n/a |");
});
