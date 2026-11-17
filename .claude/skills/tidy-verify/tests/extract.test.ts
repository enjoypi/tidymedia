/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { appendFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { extractExif, listDirCounts, mergeIncremental, mergeParts, parseManifest, planGroups, planIncremental, spawnRunner, spawnToFiles } from "../scripts/lib/extract.ts";
import type { FileRun, FileStamp } from "../scripts/lib/extract.ts";

function filesOf(entries: [string, number][]): Map<string, string[]> {
  return new Map(
    entries.map(([dir, n]) => [
      dir,
      Array.from({ length: n }, (_, i) => `${dir}/f${String(i).padStart(2, "0")}.jpg`),
    ]),
  );
}

function allFiles(files: Map<string, string[]>): string[] {
  return [...files.values()].flat().sort();
}

test("planGroups 按文件数均衡分组且只产文件行", () => {
  const files = filesOf([["a", 7], ["b", 6], ["c", 5], ["d", 4]]);
  const groups = planGroups(files, 2);
  expect(groups.length).toBe(2);
  expect(groups.map((g) => g.length).sort((x, y) => x - y)).toEqual([11, 11]);
  expect(groups.flat().sort()).toEqual(allFiles(files));
  expect(groups.flat().some((line) => line === "a" || line === "b")).toBe(false);
});

test("planGroups 单巨目录拆成文件行且负载按文件数均衡", () => {
  const big = Array.from({ length: 20 }, (_, i) => `/s/big/f${String(i).padStart(2, "0")}.jpg`);
  const groups = planGroups(new Map([["/s/big", big]]), 4);
  expect(groups.length).toBe(4);
  expect(groups.map((g) => g.length).sort((x, y) => x - y)).toEqual([5, 5, 5, 5]);
  const lines = groups.flat();
  expect([...lines].sort()).toEqual([...big].sort());
  expect(lines.every((l) => l !== "/s/big")).toBe(true);
});

test("planGroups 小目录也展开为文件行", () => {
  const files = filesOf([["/s/a", 9], ["/s/b", 2], ["/s/c", 2]]);
  const groups = planGroups(files, 3);
  expect(groups.length).toBe(3);
  const lines = groups.flat();
  for (const dir of ["/s/a", "/s/b", "/s/c"]) {
    expect(lines).not.toContain(dir);
  }
  expect([...lines].sort()).toEqual(allFiles(files));
  expect(groups.map((g) => g.length).sort((x, y) => x - y)).toEqual([4, 4, 5]);
});

test("planGroups 组数不超过文件总数且不产空组", () => {
  const groups = planGroups(filesOf([["a", 3], ["b", 1]]), 8);
  expect(groups.length).toBeLessThanOrEqual(4);
  expect(groups.every((g) => g.length > 0)).toBe(true);
  expect(groups.flat().sort()).toEqual(allFiles(filesOf([["a", 3], ["b", 1]])));
});

test("planGroups 无文件返回空", () => {
  expect(planGroups(new Map(), 4)).toEqual([]);
});

test("planGroups 并发数小于 1 按 1 处理", () => {
  expect(planGroups(filesOf([["a", 1], ["b", 1]]), 0)).toEqual([["a/f00.jpg", "b/f00.jpg"]]);
});

test("listDirCounts 返回每个目录的文件清单并跳过隐藏目录", async () => {
  const root = mkdtempSync(`${tmpdir()}/tv-list-`).replaceAll("\\", "/");
  try {
    await Bun.write(`${root}/top.jpg`, "x");
    await Bun.write(`${root}/中文 目录/a.jpg`, "x");
    await Bun.write(`${root}/中文 目录/b.jpg`, "x");
    await Bun.write(`${root}/中文 目录/子/c.mov`, "x");
    await Bun.write(`${root}/.hidden/d.jpg`, "x");
    const files = await listDirCounts(root);
    const norm = [...files.entries()].map(([d, l]) => [d, [...l].sort()] as const).sort();
    expect(norm).toEqual([
      [root, [`${root}/top.jpg`]],
      [`${root}/中文 目录`, [`${root}/中文 目录/a.jpg`, `${root}/中文 目录/b.jpg`].sort()],
      [`${root}/中文 目录/子`, [`${root}/中文 目录/子/c.mov`]],
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
    const index = calls.push(r);
    r.onSpawn(100 + index);
    const dirs = (await Bun.file(r.cmd[r.cmd.length - 1]).text()).trim().split("\n");
    await Bun.write(r.out, dirs.map((d) => `${d}\t-\r\n`).join(""));
    await Bun.write(r.err, `e${dirs.length}`);
    return index === 1 ? 1 : 0;
  };
  try {
    const files = filesOf([["D:/s/a", 5], ["D:/s/b", 3], ["D:/s/c", 2]]);
    const r = await extractExif({
      bin: "exiftool", fmt: "F", counts: files, work, concurrency: 2, run,
      tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`, onSpawn: (pid) => pids.push(pid),
    });
    expect(calls.length).toBe(2);
    expect(calls[0].cmd.slice(0, 7)).toEqual(["exiftool", "-charset", "filename=utf8", "-q", "-T", "-p", "F"]);
    expect(calls[0].cmd[7]).toBe("-@");
    expect(calls[0].out).toBe(`${work}/exif-0.tsv`);
    expect(pids.sort()).toEqual([101, 102]);
    expect(r).toEqual({ rows: 10, exit: 1, groups: 2, fast: 0 });
    const tsv = await Bun.file(`${work}/exif.tsv`).text();
    expect(tsv.split("\n").filter((l) => l !== "").sort()).toEqual(allFiles(files).map((p) => `${p}\t-`).sort());
    const err = await Bun.file(`${work}/exif.tsv.err`).text();
    expect(err).toBe("e5\ne5\n");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("listDirCounts 白名单只保留媒体扩展名且忽略大小写与空目录", async () => {
  const root = mkdtempSync(`${tmpdir()}/tv-ext-`).replaceAll("\\", "/");
  try {
    await Bun.write(`${root}/top.JPG`, "x");
    await Bun.write(`${root}/note.txt`, "x");
    await Bun.write(`${root}/LICENSE`, "x");
    await Bun.write(`${root}/中文 目录/a.mov`, "x");
    await Bun.write(`${root}/中文 目录/b.mkv`, "x");
    await Bun.write(`${root}/无媒体/c.txt`, "x");
    const files = await listDirCounts(root, ["jpg", "mov"]);
    const norm = [...files.entries()].map(([d, l]) => [d, [...l].sort()] as const).sort();
    expect(norm).toEqual([
      [root, [`${root}/top.JPG`]],
      [`${root}/中文 目录`, [`${root}/中文 目录/a.mov`]],
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("listDirCounts 白名单为空时保留全部文件", async () => {
  const root = mkdtempSync(`${tmpdir()}/tv-ext-all-`).replaceAll("\\", "/");
  try {
    await Bun.write(`${root}/top.jpg`, "x");
    await Bun.write(`${root}/note.txt`, "x");
    const files = await listDirCounts(root, []);
    expect(files.get(root)?.length).toBe(2);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("extractExif 单巨目录拆分时 argfile 写文件清单且参数文件内容完整", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-split-`).replaceAll("\\", "/");
  const calls: FileRun[] = [];
  const run = async (r: FileRun): Promise<number> => {
    calls.push(r);
    r.onSpawn(200 + calls.length);
    const lines = (await Bun.file(r.cmd[r.cmd.length - 1]).text()).trim().split("\n");
    await Bun.write(r.out, lines.map((l) => `${l}\t-\r\n`).join(""));
    await Bun.write(r.err, "");
    return 0;
  };
  try {
    const big = Array.from({ length: 12 }, (_, i) => `D:/s/中文 目录/img ${i}.jpg`);
    const r = await extractExif({
      bin: "exiftool", fmt: "F", counts: new Map([["D:/s/中文 目录", big]]), work,
      concurrency: 3, run, tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`, onSpawn: () => {},
    });
    expect(r).toEqual({ rows: 12, exit: 0, groups: 3, fast: 0 });
    const argLines: string[] = [];
    for (const c of calls) {
      argLines.push(...(await Bun.file(c.cmd[c.cmd.length - 1]).text()).trim().split("\n"));
    }
    expect(calls.length).toBe(3);
    expect([...argLines].sort()).toEqual([...big].sort());
    const tsv = await Bun.file(`${work}/exif.tsv`).text();
    expect(tsv.split("\n").filter((l) => l !== "").length).toBe(12);
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
    expect(r).toEqual({ rows: 0, exit: 0, groups: 0, fast: 0 });
    expect(await Bun.file(`${work}/t`).text()).toBe("");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 按扩展名把文件拆到 fast 与 full 组", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-fast-`).replaceAll("\\", "/");
  const calls: FileRun[] = [];
  const run = async (r: FileRun): Promise<number> => {
    calls.push(r);
    const lines = (await Bun.file(r.cmd[r.cmd.length - 1]).text()).trim().split("\n").filter((l) => l !== "");
    await Bun.write(r.out, lines.map((l) => `${l}\t-\n`).join(""));
    await Bun.write(r.err, "");
    return 0;
  };
  const argLines = async (c: FileRun): Promise<string[]> =>
    (await Bun.file(c.cmd[c.cmd.length - 1]).text()).trim().split("\n").filter((l) => l !== "");
  try {
    const counts = new Map([
      ["/s/d", ["/s/d/a.jpg", "/s/d/b.jpg", "/s/d/c.heic", "/s/d/v.mov", "/s/d/w.mp4"]],
    ]);
    const r = await extractExif({
      bin: "exiftool", fmt: "F", counts, work, concurrency: 3, run,
      fastExtensions: ["jpg", "heic"],
      tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`, onSpawn: () => {},
    });
    expect(r).toEqual({ rows: 5, exit: 0, groups: 3, fast: 3 });
    const fastCalls = calls.filter((c) => c.cmd.includes("-fast"));
    const fullCalls = calls.filter((c) => !c.cmd.includes("-fast"));
    expect(fastCalls.length).toBeGreaterThan(0);
    expect(fullCalls.length).toBeGreaterThan(0);
    expect((await Promise.all(fastCalls.map(argLines))).flat().sort()).toEqual([
      "/s/d/a.jpg", "/s/d/b.jpg", "/s/d/c.heic",
    ]);
    expect((await Promise.all(fullCalls.map(argLines))).flat().sort()).toEqual([
      "/s/d/v.mov", "/s/d/w.mp4",
    ]);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 无 fast 文件时不加 -fast", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-nofast-`).replaceAll("\\", "/");
  const calls: FileRun[] = [];
  const run = async (r: FileRun): Promise<number> => {
    calls.push(r);
    await Bun.write(r.out, "");
    await Bun.write(r.err, "");
    return 0;
  };
  try {
    const r = await extractExif({
      bin: "exiftool", fmt: "F", counts: new Map([["/s/d", ["/s/d/v.mov", "/s/d/w.mp4"]]]),
      work, concurrency: 2, run, fastExtensions: ["jpg"],
      tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`, onSpawn: () => {},
    });
    expect(r).toEqual({ rows: 0, exit: 0, groups: 2, fast: 0 });
    expect(calls.every((c) => !c.cmd.includes("-fast"))).toBe(true);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 清空分片，避免复用 work 目录时追加旧行", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-stale-`).replaceAll("\\", "/");
  await Bun.write(`${work}/exif-0.tsv`, "OLD\t-\n");
  await Bun.write(`${work}/exif-0.err`, "olderr\n");
  const run = async (r: FileRun): Promise<number> => {
    appendFileSync(r.out, "NEW\t-\n");
    appendFileSync(r.err, "");
    return 0;
  };
  try {
    const r = await extractExif({
      bin: "x", fmt: "F", counts: new Map([["/d", ["/d/a.jpg"]]]), work, concurrency: 1, run,
      tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`, onSpawn: () => {},
    });
    expect(r.rows).toBe(1);
    expect(await Bun.file(`${work}/exif.tsv`).text()).toBe("NEW\t-\n");
    expect(await Bun.file(`${work}/exif.tsv.err`).text()).toBe("");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

function stamp(size: number): FileStamp {
  return { size, mtimeMs: 10, ctimeMs: 10 };
}

test("planIncremental 只重抽新增或文件属性变化的文件", () => {
  const previous = { "/a": stamp(1), "/b": stamp(2) };
  const current = new Map([
    ["/a", stamp(1)],
    ["/b", { size: 3, mtimeMs: 10, ctimeMs: 10 }],
    ["/c", stamp(4)],
  ]);
  expect(planIncremental(current, previous)).toEqual({ retry: ["/b", "/c"], stale: [] });
});

test("planIncremental 标记已消失的旧文件", () => {
  const current = new Map([["/a", stamp(1)]]);
  expect(planIncremental(current, { "/a": stamp(1), "/b": stamp(2) })).toEqual({
    retry: [],
    stale: ["/b"],
  });
});

test("mergeIncremental 保留未变行、替换重抽行并丢弃消失行", () => {
  expect(
    mergeIncremental("/a\t1\n/b\t2\n/c\t3\n", "/b\t9\n/d\t4\n", new Set(["/b"]), new Set(["/c"])),
  ).toBe("/a\t1\n/b\t9\n/d\t4\n");
});

test("mergeIncremental 处理 CRLF 与空行", () => {
  expect(mergeIncremental("/a\t1\r\n\r\n", "/b\t2\r\n", new Set(), new Set())).toBe("/a\t1\n/b\t2\n");
});

test("parseManifest 只接受合法的路径到属性映射", () => {
  expect(parseManifest('{"/a":{"size":1,"mtimeMs":2,"ctimeMs":3}}')).toEqual({
    "/a": { size: 1, mtimeMs: 2, ctimeMs: 3 },
  });
  expect(parseManifest("{oops")).toBeNull();
  expect(parseManifest("[]")).toBeNull();
  expect(parseManifest('{"/a":{"size":"x"}}')).toBeNull();
});
