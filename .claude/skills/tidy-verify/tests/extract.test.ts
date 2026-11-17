/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { closeSync, mkdtempSync, openSync, rmSync, writeSync } from "node:fs";
import { tmpdir } from "node:os";
import {
  Chunker,
  boundedQueue,
  extractExif,
  filterManifest,
  manifestLine,
  mergeIncremental,
  mergeParts,
  parseManifest,
  pickRows,
  sameStamp,
  spawnRunner,
  spawnToFiles,
  walkMedia,
} from "../scripts/lib/extract.ts";
import type { FileRun, FileStamp } from "../scripts/lib/extract.ts";

function pathsOf(list: string[]): AsyncGenerator<string> {
  return (async function* () {
    for (const p of list) {
      yield p;
    }
  })();
}

function collect(run: FileRun): Promise<string[]> {
  return Bun.file(run.cmd[run.cmd.length - 1]).text().then((t) => t.trim().split("\n").filter((l) => l !== ""));
}

const writer = async (r: FileRun): Promise<number> => {
  const lines = await collect(r);
  await Bun.write(r.out, lines.map((l) => `${l}\t-\n`).join(""));
  await Bun.write(r.err, "");
  return 0;
};

test("walkMedia 流式产出白名单媒体并按扩展名忽略大小写", async () => {
  const root = mkdtempSync(`${tmpdir()}/tv-walk-`).replaceAll("\\", "/");
  try {
    await Bun.write(`${root}/top.JPG`, "x");
    await Bun.write(`${root}/note.txt`, "x");
    await Bun.write(`${root}/LICENSE`, "x");
    await Bun.write(`${root}/中文 目录/a.mov`, "xy");
    await Bun.write(`${root}/中文 目录/b.mkv`, "x");
    await Bun.write(`${root}/.hidden/d.jpg`, "x");
    const got: [string, number][] = [];
    for await (const item of walkMedia(root, ["jpg", "mov"])) {
      got.push([item.path, item.stamp.size]);
    }
    got.sort();
    expect(got).toEqual([
      [`${root}/top.JPG`, 1],
      [`${root}/中文 目录/a.mov`, 2],
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("walkMedia 白名单为空表示全部文件，stat 失败的文件跳过", async () => {
  const root = mkdtempSync(`${tmpdir()}/tv-walk-all-`).replaceAll("\\", "/");
  try {
    await Bun.write(`${root}/top.jpg`, "x");
    await Bun.write(`${root}/note.txt`, "x");
    const all: string[] = [];
    for await (const item of walkMedia(root, [])) {
      all.push(item.path);
    }
    expect(all.length).toBe(2);
    const none: string[] = [];
    for await (const item of walkMedia(root, [], () => null)) {
      none.push(item.path);
    }
    expect(none).toEqual([]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Chunker 攒满 size 才吐 chunk，fast 与完整扫描分开打包", () => {
  const c = new Chunker(2, ["jpg"]);
  expect(c.push("/d/a.jpg")).toBeNull();
  const first = c.push("/d/b.jpg");
  expect(first).toEqual({ seq: 0, fast: true, paths: ["/d/a.jpg", "/d/b.jpg"] });
  const mov = c.push("/d/v.mov");
  expect(mov).toBeNull();
  expect(c.push("/d/w.mp4")).toEqual({ seq: 1, fast: false, paths: ["/d/v.mov", "/d/w.mp4"] });
  expect(c.push("/d/last.jpg")).toBeNull();
  expect(c.finish()).toEqual([{ seq: 2, fast: true, paths: ["/d/last.jpg"] }]);
  expect(c.finish()).toEqual([]);
  expect([c.chunks, c.fastFiles]).toEqual([3, 3]);
});

test("Chunker size 小于 1 按 1 处理", () => {
  const c = new Chunker(0, []);
  expect(c.push("/d/a")).toEqual({ seq: 0, fast: false, paths: ["/d/a"] });
});

test("boundedQueue 满缓冲时挂起生产者，取出后恢复", async () => {
  const q = boundedQueue<number>(1);
  await q.push(0);
  let released = false;
  const slow = q.push(1).then(() => {
    released = true;
  });
  await Bun.sleep(5);
  expect(released).toBe(false);
  expect(await q.take()).toBe(0);
  await slow;
  expect(released).toBe(true);
  expect(await q.take()).toBe(1);
});

test("boundedQueue seal 唤醒等待中的生产者与消费者", async () => {
  const q = boundedQueue<number>(1);
  await q.push(0);
  const pending = q.push(1);
  q.seal();
  await pending;
  expect(await q.take()).toBe(0);
  expect(await q.take()).toBeNull();
  expect(await q.take()).toBeNull();
});

test("extractExif 按 chunk 落盘再按序号合并，回报行数与最大退出码", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-ex-`).replaceAll("\\", "/");
  const calls: FileRun[] = [];
  const run = async (r: FileRun): Promise<number> => {
    const index = calls.push(r);
    const lines = await collect(r);
    await Bun.write(r.out, lines.map((l) => `${l}\t-\r\n`).join(""));
    await Bun.write(r.err, `e${lines.length}`);
    return index === 1 ? 1 : 0;
  };
  try {
    const paths = ["D:/s/a/f0.jpg", "D:/s/a/f1.jpg", "D:/s/b/f0.jpg", "D:/s/b/f1.jpg", "D:/s/c/f0.jpg"];
    const r = await extractExif({
      bin: "exiftool", fmt: "F", paths: pathsOf(paths), work, lanes: 2, chunkFiles: 2,
      fastExtensions: [], run, tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`,
      onSpawn: () => {},
    });
    expect(calls.length).toBe(3);
    expect(calls[0].cmd.slice(0, 6)).toEqual(["exiftool", "-charset", "filename=utf8", "-q", "-T", "-p"]);
    expect(calls[0].cmd[7]).toBe("-@");
    expect(r).toEqual({ exit: 1, chunks: 3, fast: 0 });
    const tsv = await Bun.file(`${work}/exif.tsv`).text();
    expect(tsv.split("\n").filter((l) => l !== "").sort()).toEqual(paths.map((p) => `${p}\t-`).sort());
    expect(await Bun.file(`${work}/exif.tsv.err`).text()).toBe("e2\ne2\ne1\n");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 同时存活的 exiftool 不超过 lane 数", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-lane-`).replaceAll("\\", "/");
  let active = 0;
  let peak = 0;
  const run = async (r: FileRun): Promise<number> => {
    active += 1;
    peak = Math.max(peak, active);
    await Bun.sleep(2);
    await Bun.write(r.out, "");
    await Bun.write(r.err, "");
    active -= 1;
    return 0;
  };
  try {
    const paths = Array.from({ length: 40 }, (_, i) => `/d/f${String(i).padStart(2, "0")}.jpg`);
    const r = await extractExif({
      bin: "x", fmt: "F", paths: pathsOf(paths), work, lanes: 3, chunkFiles: 2,
      fastExtensions: [], run, tsv: `${work}/t`, err: `${work}/e`, onSpawn: () => {},
    });
    expect(peak).toBe(3);
    expect(r).toEqual({ exit: 0, chunks: 20, fast: 0 });
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 无文件时不调用 exiftool 且产空 tsv", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-empty-`).replaceAll("\\", "/");
  let called = false;
  const run = async (): Promise<number> => {
    called = true;
    return 0;
  };
  try {
    const r = await extractExif({
      bin: "x", fmt: "F", paths: pathsOf([]), work, lanes: 4, chunkFiles: 8,
      fastExtensions: [], run, tsv: `${work}/t`, err: `${work}/e`, onSpawn: () => {},
    });
    expect(called).toBe(false);
    expect(r).toEqual({ exit: 0, chunks: 0, fast: 0 });
    expect(await Bun.file(`${work}/t`).text()).toBe("");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 分片先清空，子进程就地覆盖更短输出时不留残行", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-trunc-`).replaceAll("\\", "/");
  const staleRow = "OLD\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n";
  await Bun.write(`${work}/exif-c0.tsv`, staleRow);
  await Bun.write(`${work}/exif-c0.err`, "OLDERR\r\n");
  // 复现子进程 stdout 的 r+ 语义：从偏移 0 覆盖，不截断文件
  const run = async (r: FileRun): Promise<number> => {
    const fd = openSync(r.out, "r+");
    writeSync(fd, Buffer.from("/d/a.jpg\t-\n"));
    closeSync(fd);
    const ef = openSync(r.err, "r+");
    writeSync(ef, Buffer.from("e1\n"));
    closeSync(ef);
    return 0;
  };
  try {
    const r = await extractExif({
      bin: "x", fmt: "F", paths: pathsOf(["/d/a.jpg"]), work, lanes: 1, chunkFiles: 4,
      fastExtensions: [], run, tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`,
      onSpawn: () => {},
    });
    expect(r).toEqual({ exit: 0, chunks: 1, fast: 0 });
    expect(await Bun.file(`${work}/exif.tsv`).text()).toBe("/d/a.jpg\t-\n");
    expect(await Bun.file(`${work}/exif.tsv.err`).text()).toBe("e1\n");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 只合并本轮序号内的分片，忽略上轮残留", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-stale-`).replaceAll("\\", "/");
  await Bun.write(`${work}/exif-c9.tsv`, "OLD\t-\n");
  await Bun.write(`${work}/exif-c9.err`, "olderr\n");
  try {
    const r = await extractExif({
      bin: "x", fmt: "F", paths: pathsOf(["/d/a.jpg"]), work, lanes: 1, chunkFiles: 4,
      fastExtensions: [], run: writer, tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`,
      onSpawn: () => {},
    });
    expect(r).toEqual({ exit: 0, chunks: 1, fast: 0 });
    expect(await Bun.file(`${work}/exif.tsv`).text()).toBe("/d/a.jpg\t-\n");
    expect(await Bun.file(`${work}/exif.tsv.err`).text()).toBe("");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("extractExif 按扩展名把文件分进带与不带 -fast 的 chunk", async () => {
  const work = mkdtempSync(`${tmpdir()}/tv-fast-`).replaceAll("\\", "/");
  const calls: FileRun[] = [];
  const run = async (r: FileRun): Promise<number> => {
    calls.push(r);
    return writer(r);
  };
  try {
    const paths = ["/s/d/a.jpg", "/s/d/v.mov", "/s/d/b.heic", "/s/d/w.mp4", "/s/d/c.jpg"];
    const r = await extractExif({
      bin: "exiftool", fmt: "F", paths: pathsOf(paths), work, lanes: 2, chunkFiles: 2,
      fastExtensions: ["jpg", "heic"], run, tsv: `${work}/exif.tsv`, err: `${work}/exif.tsv.err`,
      onSpawn: () => {},
    });
    expect(r).toEqual({ exit: 0, chunks: 3, fast: 3 });
    const fast = calls.filter((c) => c.cmd.includes("-fast"));
    const full = calls.filter((c) => !c.cmd.includes("-fast"));
    expect((await Promise.all(fast.map(collect))).flat().sort()).toEqual([
      "/s/d/a.jpg", "/s/d/b.heic", "/s/d/c.jpg",
    ]);
    expect((await Promise.all(full.map(collect))).flat().sort()).toEqual(["/s/d/v.mov", "/s/d/w.mp4"]);
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

function stamp(size: number): FileStamp {
  return { size, mtimeMs: 10, ctimeMs: 10 };
}

test("manifestLine 与 parseManifest 往返同一份属性清单", () => {
  const text = `${manifestLine("/d/中文 目录/a.jpg", stamp(7))}${manifestLine("/d/b.mov", { size: 0, mtimeMs: 1, ctimeMs: 2 })}`;
  expect(parseManifest(text)).toEqual(
    new Map<string, FileStamp>([["/d/中文 目录/a.jpg", stamp(7)], ["/d/b.mov", { size: 0, mtimeMs: 1, ctimeMs: 2 }]]),
  );
});

test("parseManifest 丢弃列数不足或属性非数字的坏行", () => {
  expect(parseManifest("/a\t1\t2\t3\nbroken\n/x\ty\tz\tw\n/b\t4\t5\t6\n")).toEqual(
    new Map<string, FileStamp>([["/a", { size: 1, mtimeMs: 2, ctimeMs: 3 }], ["/b", { size: 4, mtimeMs: 5, ctimeMs: 6 }]]),
  );
});

test("sameStamp 只在 size 与两个时间戳全等时判未变", () => {
  expect(sameStamp(stamp(1), stamp(1))).toBe(true);
  expect(sameStamp(stamp(1), { size: 1, mtimeMs: 11, ctimeMs: 10 })).toBe(false);
});

test("filterManifest 剔除重抽却无产出的文件，保留复用与成功行", () => {
  const lines = [
    manifestLine("/d/keep.jpg", stamp(1)),
    manifestLine("/d/ok.jpg", stamp(2)),
    manifestLine("/d/bad.mov", stamp(0)),
  ];
  const fresh = "/d/ok.jpg\t2024:01:01\n";
  expect(filterManifest(lines, new Set(["/d/ok.jpg", "/d/bad.mov"]), fresh)).toBe(
    `${lines[0]}${lines[1]}`,
  );
  expect(filterManifest(lines, new Set(), fresh)).toBe(lines.join(""));
});

test("pickRows 丢弃路径列不在本轮清单内的野行", () => {
  const fed = new Set(["/d/a.jpg"]);
  expect(pickRows("/d/a.jpg\t2024:01\r\n\r\n-\t-\t-\r\n", fed)).toBe("/d/a.jpg\t2024:01\n");
  expect(pickRows("", fed)).toBe("");
});

test("mergeIncremental 保留未变行、替换重抽行并丢弃消失行", () => {
  expect(
    mergeIncremental("/a\t1\n/b\t2\n/c\t3\n", "/b\t9\n/d\t4\n", new Set(["/b"]), new Set(["/c"])),
  ).toBe("/a\t1\n/b\t9\n/d\t4\n");
});

test("mergeIncremental 处理 CRLF 与空行", () => {
  expect(mergeIncremental("/a\t1\r\n\r\n", "/b\t2\r\n", new Set(), new Set())).toBe("/a\t1\n/b\t2\n");
});
