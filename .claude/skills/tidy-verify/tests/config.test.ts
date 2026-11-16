/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { loadConfig, resolveBin, workPath } from "../scripts/lib/config.ts";
import type { SkillConfig } from "../scripts/lib/config.ts";

const cfg: SkillConfig = {
  workDir: "/tmp/tm",
  tidymediaBin: "target/release/tidymedia",
  exiftoolBin: "bin/exiftool/exiftool",
  exifTsv: "exif.tsv",
  verifyReport: "verify.json",
  exiftoolTsvP: "a\tb",
  exiftoolConcurrency: 16,
  exiftoolMemMb: 300,
  exiftoolPids: "exiftool-pids.txt",
  mediaExtensions: ["jpg"],
};

test("loadConfig 读取 skill config.yaml 全部字段", async () => {
  const c = await loadConfig("config.yaml");
  expect(c.exifTsv).toBe("exif.tsv");
  expect(c.exiftoolConcurrency).toBe(16);
  expect(c.exiftoolMemMb).toBe(300);
  expect(c.exiftoolPids).toBe("exiftool-pids.txt");
  expect(c.exiftoolTsvP.split("\t").length).toBe(8);
  expect(c.mediaExtensions.length).toBeGreaterThan(0);
  expect(c.mediaExtensions).toContain("jpg");
  expect(c.mediaExtensions).toContain("mp4");
  expect(c.mediaExtensions).not.toContain("txt");
  expect(c.mediaExtensions).not.toContain("fpx");
  expect(c.mediaExtensions).toEqual([...new Set(c.mediaExtensions)]);
  expect(c.mediaExtensions.every((e) => e === e.toLowerCase() && !e.includes("."))).toBe(
    true,
  );
});

test("loadConfig 缺 media_extensions 时白名单为空", async () => {
  const dir = mkdtempSync(`${tmpdir()}/tv-cfg-`);
  try {
    const p = `${dir}/config.yaml`;
    writeFileSync(p, "exiftool_concurrency: 4\n");
    expect((await loadConfig(p.replaceAll("\\", "/"))).mediaExtensions).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

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
