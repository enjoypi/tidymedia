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
  const share = Math.ceil(total / Math.min(Math.max(concurrency, 1), total));
  const items: { line: string; weight: number }[] = [];
  for (const [dir, list] of files) {
    if (list.length > share) {
      for (const path of list) {
        items.push({ line: path, weight: 1 });
      }
    } else {
      items.push({ line: dir, weight: list.length });
    }
  }
  const k = Math.min(Math.max(concurrency, 1), items.length);
  const groups = Array.from({ length: k }, () => ({ lines: [] as string[], load: 0 }));
  for (const item of [...items].sort((a, b) => b.weight - a.weight)) {
    const lightest = groups.reduce((min, g) => (g.load < min.load ? g : min));
    lightest.lines.push(item.line);
    lightest.load += item.weight;
  }
  return groups.map((g) => g.lines);
}

export async function listDirCounts(root: string): Promise<Map<string, string[]>> {
  const base = root.replaceAll("\\", "/").replace(/\/+$/, "");
  const files = new Map<string, string[]>();
  const glob = new Bun.Glob("**/*");
  for await (const rel of glob.scan({ cwd: base, onlyFiles: true, dot: false })) {
    const norm = rel.replaceAll("\\", "/");
    const cut = norm.lastIndexOf("/");
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
}

export interface ExtractResult {
  rows: number;
  exit: number;
  groups: number;
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

export async function extractExif(opts: ExtractOptions): Promise<ExtractResult> {
  const groups = planGroups(opts.counts, opts.concurrency);
  const part = (i: number, ext: string) => `${opts.work}/exif-${i}.${ext}`;
  const exits = await Promise.all(
    groups.map(async (lines, i) => {
      const argfile = `${opts.work}/exif-args-${i}.txt`;
      await Bun.write(argfile, `${lines.join("\n")}\n`);
      return opts.run({
        cmd: [opts.bin, "-charset", "filename=utf8", "-q", "-T", "-p", opts.fmt, "-@", argfile],
        out: part(i, "tsv"),
        err: part(i, "err"),
        onSpawn: opts.onSpawn,
      });
    }),
  );
  const rows = await mergeParts(groups.map((_, i) => part(i, "tsv")), opts.tsv);
  await mergeParts(groups.map((_, i) => part(i, "err")), opts.err);
  return { rows, exit: exits.reduce((max, e) => Math.max(max, e), 0), groups: groups.length };
}
