/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { extractExif, listDirCounts, planGroups, spawnRunner } from "../scripts/lib/extract.ts";
import type { ExecResult } from "../scripts/lib/extract.ts";

const enc = new TextEncoder();

function sums(groups: string[][], counts: Map<string, number>): number[] {
  return groups.map((g) => g.reduce((s, d) => s + (counts.get(d) ?? 0), 0));
}

test("planGroups 按文件数均衡分组且每个目录恰好出现一次", () => {
  const counts = new Map([["a", 7], ["b", 6], ["c", 5], ["d", 4]]);
  const groups = planGroups(counts, 2);
  expect(groups.length).toBe(2);
  expect(sums(groups, counts).sort((x, y) => x - y)).toEqual([11, 11]);
  expect(groups.flat().sort()).toEqual(["a", "b", "c", "d"]);
});

test("planGroups 组数大于目录数时不产空组", () => {
  const counts = new Map([["a", 3], ["b", 1]]);
  expect(planGroups(counts, 8).length).toBe(2);
});

test("planGroups 无目录返回空", () => {
  expect(planGroups(new Map(), 4)).toEqual([]);
});

test("planGroups 并发数小于 1 按 1 处理", () => {
  const counts = new Map([["a", 1], ["b", 1]]);
  expect(planGroups(counts, 0)).toEqual([["a", "b"]]);
});

test("listDirCounts 统计每个目录直属文件数并跳过隐藏目录", async () => {
  const root = mkdtempSync(`${tmpdir()}/tv-list-`).replaceAll("\\", "/");
  try {
    await Bun.write(`${root}/top.jpg`, "x");
    await Bun.write(`${root}/中文 目录/a.jpg`, "x");
    await Bun.write(`${root}/中文 目录/b.jpg`, "x");
    await Bun.write(`${root}/中文 目录/子/c.mov`, "x");
    await Bun.write(`${root}/.hidden/d.jpg`, "x");
    const counts = await listDirCounts(root);
    expect([...counts.entries()].sort()).toEqual([
      [root, 1],
      [`${root}/中文 目录`, 2],
      [`${root}/中文 目录/子`, 1],
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("extractExif 每组写 UTF-8 argfile 并行调用，拼接输出取最大退出码", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-ex-`).replaceAll("\\", "/");
  const calls: string[][] = [];
  const run = async (cmd: string[]): Promise<ExecResult> => {
    calls.push(cmd);
    const argfile = cmd[cmd.length - 1];
    const dirs = (await Bun.file(argfile).text()).trim().split("\n");
    const rows = dirs.map((d) => `${d}/f.jpg\t-\r\n`).join("");
    return { out: enc.encode(rows), err: enc.encode(`e${dirs.length}\n`), exit: dirs.length === 1 ? 1 : 0 };
  };
  try {
    const counts = new Map([["D:/s/a", 5], ["D:/s/b", 3], ["D:/s/c", 2]]);
    const r = await extractExif({ bin: "exiftool", fmt: "F", counts, work, concurrency: 2, run });
    expect(calls.length).toBe(2);
    expect(calls[0].slice(0, 7)).toEqual(["exiftool", "-charset", "filename=utf8", "-q", "-T", "-p", "F"]);
    expect(calls[0][7]).toBe("-@");
    expect(r.groups).toBe(2);
    expect(r.exit).toBe(1);
    expect(r.tsv.split("\n").filter((l) => l !== "").sort()).toEqual([
      "D:/s/a/f.jpg\t-",
      "D:/s/b/f.jpg\t-",
      "D:/s/c/f.jpg\t-",
    ]);
    expect(r.err).toContain("e1");
    expect(r.err).toContain("e2");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("spawnRunner 收集 stdout/stderr 与退出码", async () => {
  const r = await spawnRunner([process.execPath, "-e", "console.log('o'); console.error('e'); process.exit(3)"]);
  expect(new TextDecoder().decode(r.out)).toBe("o\n");
  expect(new TextDecoder().decode(r.err)).toBe("e\n");
  expect(r.exit).toBe(3);
});

test("extractExif 无目录时不调用 exiftool", async () => {
  let called = false;
  const run = async (): Promise<ExecResult> => {
    called = true;
    return { out: new Uint8Array(), err: new Uint8Array(), exit: 0 };
  };
  const r = await extractExif({ bin: "x", fmt: "F", counts: new Map(), work: tmpdir(), concurrency: 4, run });
  expect(called).toBe(false);
  expect(r).toEqual({ tsv: "", err: "", exit: 0, groups: 0 });
});
