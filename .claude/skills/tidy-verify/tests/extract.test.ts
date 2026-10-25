/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { extractExif, listDirCounts, mergeParts, planGroups, spawnRunner, spawnToFiles } from "../scripts/lib/extract.ts";
import type { FileRun } from "../scripts/lib/extract.ts";

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

test("extractExif 每组输出落盘再逐组合并，回报 pid、行数与最大退出码", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-ex-`).replaceAll("\\", "/");
  const calls: FileRun[] = [];
  const pids: number[] = [];
  const run = async (r: FileRun): Promise<number> => {
    calls.push(r);
    r.onSpawn(100 + calls.length);
    const dirs = (await Bun.file(r.cmd[r.cmd.length - 1]).text()).trim().split("\n");
    await Bun.write(r.out, dirs.map((d) => `${d}/f.jpg\t-\r\n`).join(""));
    await Bun.write(r.err, `e${dirs.length}`);
    return dirs.length === 1 ? 1 : 0;
  };
  try {
    const counts = new Map([["D:/s/a", 5], ["D:/s/b", 3], ["D:/s/c", 2]]);
    const r = await extractExif({
      bin: "exiftool", fmt: "F", counts, work, concurrency: 2, run,
      tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`, onSpawn: (pid) => pids.push(pid),
    });
    expect(calls.length).toBe(2);
    expect(calls[0].cmd.slice(0, 7)).toEqual(["exiftool", "-charset", "filename=utf8", "-q", "-T", "-p", "F"]);
    expect(calls[0].cmd[7]).toBe("-@");
    expect(calls[0].out).toBe(`${work}/exif-0.tsv`);
    expect(pids.sort()).toEqual([101, 102]);
    expect(r).toEqual({ rows: 3, exit: 1, groups: 2 });
    const tsv = await Bun.file(`${work}/exif.tsv`).text();
    expect(tsv.split("\n").filter((l) => l !== "").sort()).toEqual([
      "D:/s/a/f.jpg\t-",
      "D:/s/b/f.jpg\t-",
      "D:/s/c/f.jpg\t-",
    ]);
    const err = await Bun.file(`${work}/exif.tsv.err`).text();
    expect(err).toContain("e1\n");
    expect(err).toContain("e2\n");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("mergeParts 按 GBK 兜底解码非 UTF-8 分片", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-merge-`).replaceAll("\\", "/");
  try {
    await Bun.write(`${work}/a`, new Uint8Array([0xd6, 0xd0, 0x0d, 0x0a]));
    await Bun.write(`${work}/b`, "");
    expect(await mergeParts([`${work}/a`, `${work}/b`], `${work}/out`)).toBe(1);
    expect(await Bun.file(`${work}/out`).text()).toBe("中\n");
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

test("spawnToFiles 输出直写文件并回报 pid 与退出码", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-spawn-`).replaceAll("\\", "/");
  let pid = 0;
  try {
    const exit = await spawnToFiles({
      cmd: [process.execPath, "-e", "console.log('o'); console.error('e'); process.exit(3)"],
      out: `${work}/o`, err: `${work}/e`, onSpawn: (p) => { pid = p; },
    });
    expect(exit).toBe(3);
    expect(pid).toBeGreaterThan(0);
    expect(await Bun.file(`${work}/o`).text()).toBe("o\n");
    expect(await Bun.file(`${work}/e`).text()).toBe("e\n");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 无目录时不调用 exiftool 且产空 tsv", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-empty-`).replaceAll("\\", "/");
  let called = false;
  const run = async (): Promise<number> => {
    called = true;
    return 0;
  };
  try {
    const r = await extractExif({
      bin: "x", fmt: "F", counts: new Map(), work, concurrency: 4, run,
      tsv: `${work}/t`, err: `${work}/e`, onSpawn: () => {},
    });
    expect(called).toBe(false);
    expect(r).toEqual({ rows: 0, exit: 0, groups: 0 });
    expect(await Bun.file(`${work}/t`).text()).toBe("");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});
