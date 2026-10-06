/// <reference path="./bun.d.ts" />

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

export interface ExtractOptions {
  bin: string;
  fmt: string;
  counts: Map<string, number>;
  work: string;
  concurrency: number;
  run: Runner;
}

export interface ExtractResult {
  tsv: string;
  err: string;
  exit: number;
  groups: number;
}

function normalize(bytes: Uint8Array): string {
  const text = decodeExifText(bytes).replace(/\r\n/g, "\n");
  return text === "" || text.endsWith("\n") ? text : `${text}\n`;
}

export async function extractExif(opts: ExtractOptions): Promise<ExtractResult> {
  const groups = planGroups(opts.counts, opts.concurrency);
  const results = await Promise.all(
    groups.map(async (dirs, i) => {
      const argfile = `${opts.work}/exif-args-${i}.txt`;
      await Bun.write(argfile, `${dirs.join("\n")}\n`);
      return opts.run([
        opts.bin, "-charset", "filename=utf8", "-q", "-T", "-p", opts.fmt, "-@", argfile,
      ]);
    }),
  );
  return {
    tsv: results.map((r) => normalize(r.out)).join(""),
    err: results.map((r) => normalize(r.err)).join(""),
    exit: results.reduce((max, r) => Math.max(max, r.exit), 0),
    groups: groups.length,
  };
}
