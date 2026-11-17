/// <reference path="./bun.d.ts" />

import { appendFileSync, writeFileSync } from "node:fs";
import { decodeExifText } from "./gbk.ts";
export { spawnRunner } from "./procs.ts";
export type { ExecResult, Runner } from "./procs.ts";

export function planGroups(files: Map<string, string[]>, concurrency: number): string[][] {
  const total = [...files.values()].reduce((s, l) => s + l.length, 0);
  if (total === 0) {
    return [];
  }
  const k = Math.min(Math.max(concurrency, 1), total);
  const share = Math.ceil(total / k);
  const items: { lines: string[]; weight: number }[] = [];
  for (const [dir, list] of files) {
    if (list.length > share) {
      for (const path of list) {
        items.push({ lines: [path], weight: 1 });
      }
    } else {
      items.push({ lines: list, weight: list.length });
    }
  }
  const groups = Array.from({ length: k }, () => ({ lines: [] as string[], load: 0 }));
  for (const item of [...items].sort((a, b) => b.weight - a.weight)) {
    const lightest = groups.reduce((min, g) => (g.load < min.load ? g : min));
    lightest.lines.push(...item.lines);
    lightest.load += item.weight;
  }
  return groups.map((g) => g.lines).filter((g) => g.length > 0);
}

function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot <= 0 ? "" : name.slice(dot + 1).toLowerCase();
}

// allow 为空表示不过滤（保持旧行为：全量文件交给 exiftool）。
export async function listDirCounts(root: string, allow: string[] = []): Promise<Map<string, string[]>> {
  const keep = new Set(allow.map((e) => e.toLowerCase()));
  const base = root.replaceAll("\\", "/").replace(/\/+$/, "");
  const files = new Map<string, string[]>();
  const glob = new Bun.Glob("**/*");
  for await (const rel of glob.scan({ cwd: base, onlyFiles: true, dot: false })) {
    const norm = rel.replaceAll("\\", "/");
    const cut = norm.lastIndexOf("/");
    const name = cut < 0 ? norm : norm.slice(cut + 1);
    if (keep.size > 0 && !keep.has(extOf(name))) {
      continue;
    }
    const dir = cut < 0 ? base : `${base}/${norm.slice(0, cut)}`;
    const list = files.get(dir) ?? [];
    list.push(`${base}/${norm}`);
    files.set(dir, list);
  }
  return files;
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
  counts: Map<string, string[]>;
  work: string;
  concurrency: number;
  run: FileRunner;
  tsv: string;
  err: string;
  onSpawn: (pid: number) => void;
  fastExtensions?: string[];
}

export interface ExtractResult {
  rows: number;
  exit: number;
  groups: number;
  fast: number;
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

function partitionByFast(
  counts: Map<string, string[]>,
  fast: Set<string>,
): { fast: Map<string, string[]>; full: Map<string, string[]> } {
  const fastFiles = new Map<string, string[]>();
  const fullFiles = new Map<string, string[]>();
  for (const [dir, list] of counts) {
    for (const path of list) {
      const target = fast.has(extOf(path)) ? fastFiles : fullFiles;
      const bucket = target.get(dir) ?? [];
      bucket.push(path);
      target.set(dir, bucket);
    }
  }
  return { fast: fastFiles, full: fullFiles };
}

function countOf(files: Map<string, string[]>): number {
  return [...files.values()].reduce((s, l) => s + l.length, 0);
}

export async function extractExif(opts: ExtractOptions): Promise<ExtractResult> {
  const fastSet = new Set((opts.fastExtensions ?? []).map((e) => e.toLowerCase()));
  const split = partitionByFast(opts.counts, fastSet);
  const fastTotal = countOf(split.fast);
  const total = fastTotal + countOf(split.full);
  if (total === 0) {
    writeFileSync(opts.tsv, "");
    writeFileSync(opts.err, "");
    return { rows: 0, exit: 0, groups: 0, fast: 0 };
  }
  const k = Math.min(Math.max(opts.concurrency, 1), total);
  let fastK = 0;
  if (fastTotal > 0) {
    fastK = fastTotal === total
      ? k
      : Math.min(Math.max(Math.round((k * fastTotal) / total), 1), k - 1);
  }
  const groups: { lines: string[]; fast: boolean }[] = [
    ...planGroups(split.fast, fastK).map((lines) => ({ lines, fast: true })),
    ...planGroups(split.full, k - fastK).map((lines) => ({ lines, fast: false })),
  ];
  const part = (i: number, ext: string) => `${opts.work}/exif-${i}.${ext}`;
  const exits = await Promise.all(
    groups.map(async (group, i) => {
      const argfile = `${opts.work}/exif-args-${i}.txt`;
      const out = part(i, "tsv");
      const err = part(i, "err");
      writeFileSync(out, "");
      writeFileSync(err, "");
      await Bun.write(argfile, `${group.lines.join("\n")}\n`);
      return opts.run({
        cmd: [
          opts.bin, "-charset", "filename=utf8", "-q",
          ...(group.fast ? ["-fast"] : []),
          "-T", "-p", opts.fmt, "-@", argfile,
        ],
        out,
        err,
        onSpawn: opts.onSpawn,
      });
    }),
  );
  const rows = await mergeParts(groups.map((_, i) => part(i, "tsv")), opts.tsv);
  await mergeParts(groups.map((_, i) => part(i, "err")), opts.err);
  return { rows, exit: exits.reduce((max, e) => Math.max(max, e), 0), groups: groups.length, fast: fastTotal };
}

export interface FileStamp {
  size: number;
  mtimeMs: number;
  ctimeMs: number;
}

export type Manifest = Record<string, FileStamp>;

export interface IncrementalPlan {
  retry: string[];
  stale: string[];
}

export function planIncremental(current: Map<string, FileStamp>, previous: Manifest): IncrementalPlan {
  const retry: string[] = [];
  for (const [path, stamp] of current) {
    const prev = previous[path];
    if (
      prev === undefined ||
      prev.size !== stamp.size ||
      prev.mtimeMs !== stamp.mtimeMs ||
      prev.ctimeMs !== stamp.ctimeMs
    ) {
      retry.push(path);
    }
  }
  const stale = Object.keys(previous).filter((path) => !current.has(path));
  return { retry: retry.sort(), stale: stale.sort() };
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

export function parseManifest(text: string): Manifest | null {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return null;
  }
  const manifest: Manifest = {};
  for (const [path, value] of Object.entries(raw as Record<string, unknown>)) {
    if (typeof value !== "object" || value === null) {
      return null;
    }
    const stamp = value as Record<string, unknown>;
    if (
      typeof stamp.size !== "number" ||
      typeof stamp.mtimeMs !== "number" ||
      typeof stamp.ctimeMs !== "number"
    ) {
      return null;
    }
    manifest[path] = { size: stamp.size, mtimeMs: stamp.mtimeMs, ctimeMs: stamp.ctimeMs };
  }
  return manifest;
}
