/// <reference path="./bun.d.ts" />

import { appendFileSync, writeFileSync } from "node:fs";
import { decodeExifText } from "./gbk.ts";

export interface ExecResult {
  out: Uint8Array;
  err: Uint8Array;
  exit: number;
}

export type Runner = (cmd: string[]) => Promise<ExecResult>;

export async function spawnRunner(cmd: string[]): Promise<ExecResult> {
  const proc = Bun.spawn(cmd, { stdout: "pipe", stderr: "pipe" });
  const [out, err] = await Promise.all([
    new Response(proc.stdout).arrayBuffer(),
    new Response(proc.stderr).arrayBuffer(),
  ]);
  return { out: new Uint8Array(out), err: new Uint8Array(err), exit: await proc.exited };
}

export function planGroups(counts: Map<string, number>, concurrency: number): string[][] {
  const k = Math.min(Math.max(concurrency, 1), counts.size);
  const groups = Array.from({ length: k }, () => ({ dirs: [] as string[], load: 0 }));
  const sorted = [...counts.entries()].sort((a, b) => b[1] - a[1]);
  for (const [dir, n] of sorted) {
    const lightest = groups.reduce((min, g) => (g.load < min.load ? g : min));
    lightest.dirs.push(dir);
    lightest.load += n;
  }
  return groups.map((g) => g.dirs);
}

export async function listDirCounts(root: string): Promise<Map<string, number>> {
  const base = root.replaceAll("\\", "/").replace(/\/+$/, "");
  const counts = new Map<string, number>();
  const glob = new Bun.Glob("**/*");
  for await (const rel of glob.scan({ cwd: base, onlyFiles: true, dot: false })) {
    const norm = rel.replaceAll("\\", "/");
    const cut = norm.lastIndexOf("/");
    const dir = cut < 0 ? base : `${base}/${norm.slice(0, cut)}`;
    counts.set(dir, (counts.get(dir) ?? 0) + 1);
  }
  return counts;
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
  counts: Map<string, number>;
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
    groups.map(async (dirs, i) => {
      const argfile = `${opts.work}/exif-args-${i}.txt`;
      await Bun.write(argfile, `${dirs.join("\n")}\n`);
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
