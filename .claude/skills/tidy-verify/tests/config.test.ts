/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { resolveBin, workPath } from "../scripts/lib/config.ts";
import type { SkillConfig } from "../scripts/lib/config.ts";

const cfg: SkillConfig = {
  workDir: "/tmp/tm",
  tidymediaBin: "target/release/tidymedia",
  exiftoolBin: "bin/exiftool/exiftool",
  exifTsv: "exif.tsv",
  verifyReport: "verify.json",
  exiftoolTsvP: "a\tb",
};

test("resolveBin 命中当前平台可执行文件（非 win 不探测 .exe）", async () => {
  // cwd=repo 根：config.yaml 存在，config.yaml.exe 不存在
  const r = await resolveBin("config.yaml");
  expect(r).toBe("config.yaml");
});

test("resolveBin 不存在返回 null", async () => {
  expect(await resolveBin("/no/such/bin/xyz")).toBeNull();
});

test("workPath 拼接 work_dir 与文件名", () => {
  expect(workPath(cfg, "exif.tsv")).toBe("/tmp/tm/exif.tsv");
  expect(workPath({ ...cfg, workDir: "/tmp/tm/" }, "verify.json")).toBe(
    "/tmp/tm/verify.json",
  );
});
