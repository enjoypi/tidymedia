/// <reference path="./bun.d.ts" />

import { appendFileSync, statSync, writeFileSync } from "node:fs";
import { decodeExifText } from "./gbk.ts";
export { spawnRunner } from "./procs.ts";
export type { ExecResult, Runner } from "./procs.ts";

export interface FileStamp {
  size: number;
  mtimeMs: number;
  ctimeMs: number;
}

function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot <= 0 ? "" : name.slice(dot + 1).toLowerCase();
}

function statStamp(path: string): FileStamp | null {
  try {
    const s = statSync(path);
    return { size: s.size, mtimeMs: s.mtimeMs, ctimeMs: s.ctimeMs };
  } catch {
    return null;
  }
}

// 逐个产出媒体文件路径，不在内存里缓存整棵目录树。allow 为空表示不过滤扩展名。
export async function* walkMedia(
  root: string,
  allow: string[] = [],
  stat: (path: string) => FileStamp | null = statStamp,
): AsyncGenerator<{ path: string; stamp: FileStamp }> {
  const keep = new Set(allow.map((e) => e.toLowerCase()));
  const base = root.replaceAll("\\", "/").replace(/\/+$/, "");
  for await (const rel of new Bun.Glob("**/*").scan({ cwd: base, onlyFiles: true, dot: false })) {
    const norm = rel.replaceAll("\\", "/");
    const name = norm.slice(norm.lastIndexOf("/") + 1);
    if (keep.size > 0 && !keep.has(extOf(name))) {
      continue;
    }
    const stamp = stat(`${base}/${norm}`);
    if (stamp === null) {
      continue;
    }
    yield { path: `${base}/${norm}`, stamp };
  }
}

export interface Chunk {
  seq: number;
  fast: boolean;
  paths: string[];
}

// 按扩展名分成 -fast 与完整扫描两条待打包缓冲；攒满 size 吐出一个 chunk。
export class Chunker {
  private readonly size: number;
  chunks = 0;
  fastFiles = 0;
  private readonly fastSet: Set<string>;
  private readonly open = new Map<boolean, string[]>();

  constructor(size: number, fastExtensions: string[]) {
    this.size = Math.max(size, 1);
    this.fastSet = new Set(fastExtensions.map((e) => e.toLowerCase()));
  }

  private emit(fast: boolean, paths: string[]): Chunk {
    this.chunks += 1;
    if (fast) {
      this.fastFiles += paths.length;
    }
    return { seq: this.chunks - 1, fast, paths };
  }

  push(path: string): Chunk | null {
    const fast = this.fastSet.has(extOf(path));
    const buf = this.open.get(fast) ?? [];
    buf.push(path);
    if (buf.length < this.size) {
      this.open.set(fast, buf);
      return null;
    }
    this.open.set(fast, []);
    return this.emit(fast, buf);
  }

  finish(): Chunk[] {
    const rest: Chunk[] = [];
    for (const [fast, paths] of this.open) {
      if (paths.length > 0) {
        rest.push(this.emit(fast, paths));
      }
    }
    this.open.clear();
    return rest;
  }
}

export interface Queue<T> {
  push(item: T): Promise<void>;
  seal(): void;
  take(): Promise<T | null>;
}

// 有界队列：生产者满缓冲时挂起，形成对目录遍历的天然背压。
export function boundedQueue<T>(cap: number): Queue<T> {
  const items: T[] = [];
  const takers: ((item: T | null) => void)[] = [];
  const pushers: (() => void)[] = [];
  let sealed = false;
  return {
    async push(item) {
      while (!sealed && items.length >= cap) {
        await new Promise<void>((resolve) => pushers.push(resolve));
      }
      if (sealed) {
        return;
      }
      const taker = takers.shift();
      if (taker === undefined) {
        items.push(item);
        return;
      }
      taker(item);
    },
    seal() {
      sealed = true;
      while (pushers.length > 0) {
        (pushers.shift() as () => void)();
      }
      while (takers.length > 0) {
        (takers.shift() as (item: T | null) => void)(null);
      }
    },
    async take() {
      if (items.length > 0) {
        const item = items.shift() as T;
        if (pushers.length > 0) {
          (pushers.shift() as () => void)();
        }
        return item;
      }
      if (sealed) {
        return null;
      }
      return new Promise<T | null>((resolve) => takers.push(resolve));
    },
  };
}

export interface FileRun {
  cmd: string[];
  out: string;
  err: string;
  onSpawn: (pid: number) => void;
}

export type FileRunner = (r: FileRun) => Promise<number>;

export function spawnToFiles(r: FileRun): Promise<number> {
  const proc = Bun.spawn(r.cmd, { stdout: Bun.file(r.out), stderr: Bun.file(r.err) });
  r.onSpawn(proc.pid);
  return proc.exited;
}

export interface ExtractOptions {
  bin: string;
  fmt: string;
  paths: AsyncIterable<string>;
  work: string;
  lanes: number;
  chunkFiles: number;
  fastExtensions: string[];
  run: FileRunner;
  tsv: string;
  err: string;
  onSpawn: (pid: number) => void;
}

export interface ExtractResult {
  exit: number;
  chunks: number;
  fast: number;
}

function partPath(work: string, seq: number, kind: "tsv" | "err"): string {
  return `${work}/exif-${kind === "tsv" ? "c" : "e"}${seq}.${kind}`;
}

async function runChunk(opts: ExtractOptions, chunk: Chunk): Promise<number> {
  const argfile = `${opts.work}/exif-args-${chunk.seq}.txt`;
  const out = partPath(opts.work, chunk.seq, "tsv");
  const err = partPath(opts.work, chunk.seq, "err");
  await Bun.write(argfile, `${chunk.paths.join("\n")}\n`);
  // 子进程 stdout 就地覆盖不截断，复用同一 work 目录时必须先清空分片
  writeFileSync(out, "");
  writeFileSync(err, "");
  return opts.run({
    cmd: [
      opts.bin, "-charset", "filename=utf8", "-q",
      ...(chunk.fast ? ["-fast"] : []),
      "-T", "-p", opts.fmt, "-@", argfile,
    ],
    out,
    err,
    onSpawn: opts.onSpawn,
  });
}

async function lane(opts: ExtractOptions, queue: Queue<Chunk>, exits: number[]): Promise<void> {
  for (;;) {
    const chunk = await queue.take();
    if (chunk === null) {
      return;
    }
    exits[chunk.seq] = await runChunk(opts, chunk);
  }
}

// 生产者遍历路径流，lane 消费者抢占 chunk；chunks 数由文件数决定，与 lane 数无关。
export async function extractExif(opts: ExtractOptions): Promise<ExtractResult> {
  const lanes = Math.max(opts.lanes, 1);
  const queue = boundedQueue<Chunk>(lanes * 2);
  const chunker = new Chunker(opts.chunkFiles, opts.fastExtensions);
  const produce = (async () => {
    for await (const path of opts.paths) {
      const chunk = chunker.push(path);
      if (chunk !== null) {
        await queue.push(chunk);
      }
    }
    for (const rest of chunker.finish()) {
      await queue.push(rest);
    }
    queue.seal();
  })();
  const exits: number[] = [];
  await Promise.all([produce, ...Array.from({ length: lanes }, () => lane(opts, queue, exits))]);
  const total = chunker.chunks;
  if (total === 0) {
    writeFileSync(opts.tsv, "");
    writeFileSync(opts.err, "");
    return { exit: 0, chunks: 0, fast: 0 };
  }
  const seqs = Array.from({ length: total }, (_, seq) => seq);
  await mergeParts(seqs.map((seq) => partPath(opts.work, seq, "tsv")), opts.tsv);
  await mergeParts(seqs.map((seq) => partPath(opts.work, seq, "err")), opts.err);
  return {
    exit: seqs.reduce((max, seq) => Math.max(max, exits[seq] as number), 0),
    chunks: total,
    fast: chunker.fastFiles,
  };
}

function normalize(bytes: Uint8Array): string {
  const text = decodeExifText(bytes).replace(/\r\n/g, "\n");
  return text === "" || text.endsWith("\n") ? text : `${text}\n`;
}

export async function mergeParts(parts: string[], dest: string): Promise<number> {
  writeFileSync(dest, "");
  let rows = 0;
  for (const part of parts) {
    const text = normalize(new Uint8Array(await Bun.file(part).arrayBuffer()));
    rows += text.split("\n").length - 1;
    appendFileSync(dest, text);
  }
  return rows;
}

export type Manifest = Map<string, FileStamp>;

export function manifestLine(path: string, stamp: FileStamp): string {
  return `${path}\t${stamp.size}\t${stamp.mtimeMs}\t${stamp.ctimeMs}\n`;
}

export function parseManifest(text: string): Manifest {
  const manifest: Manifest = new Map();
  for (const line of text.split("\n")) {
    const cols = line.split("\t");
    if (cols.length !== 4) {
      continue;
    }
    const size = Number(cols[1]);
    const mtimeMs = Number(cols[2]);
    const ctimeMs = Number(cols[3]);
    if (!Number.isFinite(size) || !Number.isFinite(mtimeMs) || !Number.isFinite(ctimeMs)) {
      continue;
    }
    manifest.set(cols[0] as string, { size, mtimeMs, ctimeMs });
  }
  return manifest;
}

export function sameStamp(a: FileStamp, b: FileStamp): boolean {
  return a.size === b.size && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs;
}

// 只保留路径列属于本轮抽取清单的行：多图或损坏文件会吐出行数多于文件数，
// 多余行的路径列是占位符，留下来会污染按路径键控的增量合并与 verify 索引。
export function pickRows(text: string, fed: Set<string>): string {
  return tsvLines(text)
    .filter((line) => fed.has(linePath(line)))
    .map((line) => `${line}\n`)
    .join("");
}

// 本轮重抽过但没有产出 TSV 行的文件（读取失败等）不写进清单，下次继续重抽。
export function filterManifest(lines: string[], retried: Set<string>, freshTsv: string): string {
  if (retried.size === 0) {
    return lines.join("");
  }
  const rows = new Set<string>();
  for (const line of freshTsv.split("\n")) {
    if (line !== "") {
      rows.add(linePath(line));
    }
  }
  return lines
    .filter((line) => {
      const path = line.split("\t")[0] as string;
      return !retried.has(path) || rows.has(path);
    })
    .join("");
}

function tsvLines(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.replace(/\r$/, ""))
    .filter((line) => line !== "");
}

function linePath(line: string): string {
  const cut = line.indexOf("\t");
  return cut < 0 ? line : line.slice(0, cut);
}

export function mergeIncremental(
  previousTsv: string,
  freshTsv: string,
  retry: Set<string>,
  stale: Set<string>,
): string {
  const merged = new Map<string, string>();
  for (const line of tsvLines(previousTsv)) {
    const path = linePath(line);
    if (retry.has(path) || stale.has(path)) {
      continue;
    }
    merged.set(path, line);
  }
  for (const line of tsvLines(freshTsv)) {
    merged.set(linePath(line), line);
  }
  return [...merged.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([, line]) => `${line}\n`)
    .join("");
}
