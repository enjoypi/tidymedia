/// <reference path="./lib/bun.d.ts" />

import { mkdirSync, statSync, writeFileSync } from "node:fs";
import { freemem } from "node:os";
import { dirname } from "node:path";
import { exiftoolBin, loadConfig, tidymediaBin, workPath } from "./lib/config.ts";
import type { SkillConfig } from "./lib/config.ts";
import {
  extractExif,
  listDirCounts,
  mergeIncremental,
  parseManifest,
  planIncremental,
  spawnToFiles,
} from "./lib/extract.ts";
import type { FileStamp, Manifest } from "./lib/extract.ts";
import { cpuCount, isImageRunning, killTree, pickConcurrency, reapStale, recordPid } from "./lib/procs.ts";
import { startVerify, verifyCommand, verifyExitCode } from "./lib/verify.ts";
import type { RunningVerify } from "./lib/verify.ts";

const MB = 1024 * 1024;

function stampsOf(counts: Map<string, string[]>): Map<string, FileStamp> {
  const stamps = new Map<string, FileStamp>();
  for (const list of counts.values()) {
    for (const path of list) {
      try {
        const s = statSync(path);
        stamps.set(path, { size: s.size, mtimeMs: s.mtimeMs, ctimeMs: s.ctimeMs });
      } catch {
        continue;
      }
    }
  }
  return stamps;
}

function filterCounts(counts: Map<string, string[]>, keep: Set<string>): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const [dir, list] of counts) {
    const kept = list.filter((path) => keep.has(path));
    if (kept.length > 0) {
      out.set(dir, kept);
    }
  }
  return out;
}

async function readPrevious(manifestPath: string, tsvPath: string): Promise<Manifest | null> {
  if (!(await Bun.file(manifestPath).exists()) || !(await Bun.file(tsvPath).exists())) {
    return null;
  }
  return parseManifest(await Bun.file(manifestPath).text());
}

async function writeManifest(
  manifestPath: string,
  stamps: Map<string, FileStamp>,
  previous: Manifest | null,
  retried: Set<string>,
  exit: number,
): Promise<void> {
  if (exit !== 0) {
    if (previous !== null) {
      await Bun.file(manifestPath).delete();
    }
    return;
  }
  const next: Manifest = {};
  for (const [path, stamp] of stamps) {
    next[path] = previous !== null && !retried.has(path) ? (previous[path] ?? stamp) : stamp;
  }
  await Bun.write(manifestPath, JSON.stringify(next));
}

async function extractStep(cfg: SkillConfig, src: string): Promise<string | null> {
  const bin = await exiftoolBin(cfg);
  if (!bin) {
    console.log("exiftool_missing=1（跳过 --exif-tsv，verify 内部判定仍有效）");
    return null;
  }
  const pids = `${dirname(cfg.workDir)}/${cfg.exiftoolPids}`;
  const stale = await reapStale(pids, (pid) => isImageRunning(pid, "exiftool"), killTree);
  if (stale.length > 0) {
    console.log(`stale_exiftool_killed=${stale.join(",")}`);
  }
  const t0 = performance.now();
  const counts = await listDirCounts(src, cfg.mediaExtensions);
  const cpu = cpuCount();
  const concurrency = pickConcurrency(
    cfg.exiftoolConcurrency,
    cpu,
    cfg.taskMemoryLimitMb * MB,
    cfg.taskMemoryReserveMb * MB,
    cfg.exiftoolMemMb * MB,
  );
  const tsv = workPath(cfg, cfg.exifTsv);
  const freshTsv = workPath(cfg, "exif-fresh.tsv");
  const manifestPath = workPath(cfg, "exif.manifest.json");
  const previous = cfg.incremental && !src.toLowerCase().startsWith("smb://")
    ? await readPrevious(manifestPath, tsv)
    : null;
  const stamps = stampsOf(counts);
  const plan = previous === null
    ? { retry: [...stamps.keys()].sort(), stale: [] as string[] }
    : planIncremental(stamps, previous);
  const retried = new Set(plan.retry);
  const r = await extractExif({
    bin, fmt: cfg.exiftoolTsvP, counts: filterCounts(counts, retried), work: cfg.workDir,
    concurrency, run: spawnToFiles, tsv: freshTsv, err: `${freshTsv}.err`,
    onSpawn: (pid) => recordPid(pids, pid), fastExtensions: cfg.fastExtensions,
  });
  const fresh = await Bun.file(freshTsv).text();
  const merged = previous === null
    ? fresh
    : mergeIncremental(await Bun.file(tsv).text(), fresh, retried, new Set(plan.stale));
  await Bun.write(tsv, merged);
  await writeManifest(manifestPath, stamps, previous, retried, r.exit);
  writeFileSync(pids, "");
  const rows = merged === "" ? 0 : merged.split("\n").filter((line) => line !== "").length;
  const reused = previous === null ? 0 : stamps.size - plan.retry.length;
  console.log(
    `exif_rows=${rows} exif_groups=${r.groups} exif_exit=${r.exit} exif_fast=${r.fast} exif_reused=${reused} exif_concurrency=${concurrency} freemem_mb=${Math.round(freemem() / MB)} cpu=${cpu} extract_ms=${Math.round(performance.now() - t0)}`,
  );
  return tsv;
}

async function finishVerifyStep(
  cfg: SkillConfig,
  running: RunningVerify,
  started: number,
  tsv: string | null,
): Promise<number> {
  const input = tsv === null ? new Uint8Array() : new Uint8Array(await Bun.file(tsv).arrayBuffer());
  const sent = await running.send(input);
  const r = await running.finish();
  const summary = new TextDecoder().decode(r.out);
  await Bun.write(workPath(cfg, "summary.txt"), summary);
  await Bun.write(workPath(cfg, "verify.err"), r.err);
  console.log(summary.trimEnd());
  console.log(`verify_exit=${r.exit} verify_ms=${Math.round(performance.now() - started)}`);
  if (!sent) {
    console.log("verify_stdin=error");
  }
  return verifyExitCode(r.exit, sent);
}

async function main(args: string[]): Promise<number> {
  const [src, out, work] = args;
  if (!src || !out || !work) {
    console.error("Usage: bun run.ts <source_dir> <output_dir> <work_dir>");
    return 2;
  }
  const total = performance.now();
  const cfg = { ...(await loadConfig()), workDir: work };
  const bin = await tidymediaBin(cfg);
  if (!bin) {
    console.error(`tidymedia 缺失：${cfg.tidymediaBin}，先 just build-release`);
    return 2;
  }
  mkdirSync(work, { recursive: true });
  console.log(`work_dir=${work}`);
  const started = performance.now();
  const running = startVerify(verifyCommand(bin, src, out, "-", workPath(cfg, cfg.verifyReport)));
  const tsv = await extractStep(cfg, src);
  const code = await finishVerifyStep(cfg, running, started, tsv);
  console.log(`total_ms=${Math.round(performance.now() - total)}`);
  return code;
}

process.exit(await main(process.argv.slice(2)));
