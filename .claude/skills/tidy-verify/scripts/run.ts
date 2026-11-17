/// <reference path="./lib/bun.d.ts" />

import { mkdirSync, writeFileSync } from "node:fs";
import { freemem } from "node:os";
import { dirname } from "node:path";
import { exiftoolBin, loadConfig, tidymediaBin, workPath } from "./lib/config.ts";
import type { SkillConfig } from "./lib/config.ts";
import {
  extractExif,
  filterManifest,
  manifestLine,
  mergeIncremental,
  parseManifest,
  pickRows,
  sameStamp,
  spawnToFiles,
  walkMedia,
} from "./lib/extract.ts";
import type { Manifest } from "./lib/extract.ts";
import { cpuCount, isImageRunning, killTree, pickConcurrency, reapStale, recordPid } from "./lib/procs.ts";
import { startVerify, verifyCommand, verifyExitCode } from "./lib/verify.ts";
import type { RunningVerify } from "./lib/verify.ts";

const MB = 1024 * 1024;

interface Feed {
  paths: AsyncGenerator<string>;
  manifest: string[];
  retried: string[];
  counters: { reused: number };
}

// 边遍历边判定：size/mtime/ctime 未变的文件不进抽取队列，直接复用上一轮 TSV 行。
function makeFeed(src: string, cfg: SkillConfig, prev: Manifest): Feed {
  const manifest: string[] = [];
  const retried: string[] = [];
  const counters = { reused: 0 };
  async function* paths(): AsyncGenerator<string> {
    for await (const item of walkMedia(src, cfg.mediaExtensions)) {
      manifest.push(manifestLine(item.path, item.stamp));
      const old = prev.get(item.path);
      if (old !== undefined) {
        prev.delete(item.path);
        if (sameStamp(old, item.stamp)) {
          counters.reused += 1;
          continue;
        }
      }
      retried.push(item.path);
      yield item.path;
    }
  }
  return { paths: paths(), manifest, retried, counters };
}

async function loadPrevious(
  cfg: SkillConfig,
  src: string,
  tsv: string,
  manifestPath: string,
): Promise<{ prev: Manifest; had: boolean }> {
  if (!cfg.incremental || src.toLowerCase().startsWith("smb://")) {
    return { prev: new Map(), had: false };
  }
  if (!(await Bun.file(manifestPath).exists()) || !(await Bun.file(tsv).exists())) {
    return { prev: new Map(), had: false };
  }
  return { prev: parseManifest(await Bun.file(manifestPath).text()), had: true };
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
  const started = performance.now();
  const lap = newLap(started);
  const cpu = cpuCount();
  const lanes = pickConcurrency(
    cfg.exiftoolConcurrency,
    cpu,
    cfg.taskMemoryLimitMb * MB,
    cfg.taskMemoryReserveMb * MB,
    cfg.exiftoolMemMb * MB,
  );
  const tsv = workPath(cfg, cfg.exifTsv);
  const freshTsv = workPath(cfg, "exif-fresh.tsv");
  const manifestPath = workPath(cfg, cfg.exifManifest);
  const { prev, had } = await loadPrevious(cfg, src, tsv, manifestPath);
  const feed = makeFeed(src, cfg, prev);
  const r = await extractExif({
    bin, fmt: cfg.exiftoolTsvP, paths: feed.paths, work: cfg.workDir, lanes,
    chunkFiles: cfg.exiftoolChunkFiles, fastExtensions: cfg.fastExtensions,
    run: spawnToFiles, tsv: freshTsv, err: `${freshTsv}.err`,
    onSpawn: (pid) => recordPid(pids, pid),
  });
  lap("exiftool");
  const retried = new Set(feed.retried);
  const fresh = pickRows(await Bun.file(freshTsv).text(), retried);
  const merged = had
    ? mergeIncremental(await Bun.file(tsv).text(), fresh, retried, new Set(prev.keys()))
    : fresh;
  await Bun.write(tsv, merged);
  const rows = merged === "" ? 0 : merged.split("\n").filter((line) => line !== "").length;
  await Bun.write(manifestPath, filterManifest(feed.manifest, retried, fresh));
  writeFileSync(pids, "");
  lap("merge");
  console.log(
    `exif_rows=${rows} exif_chunks=${r.chunks} exif_exit=${r.exit} exif_fast=${r.fast} exif_reused=${feed.counters.reused} exif_lanes=${lanes} exif_retry=${feed.retried.length} freemem_mb=${Math.round(freemem() / MB)} cpu=${cpu} bun_rss_mb=${Math.round(process.memoryUsage.rss() / MB)} extract_ms=${Math.round(performance.now() - started)} laps=${lap.value()}`,
  );
  return tsv;
}

function newLap(started: number): ((name: string) => void) & { value(): string } {
  const laps: string[] = [];
  let last = started;
  const fn = (name: string) => {
    const now = performance.now();
    laps.push(`${name}:${Math.round(now - last)}`);
    last = now;
  };
  return Object.assign(fn, { value: () => laps.join(",") });
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
  // 抽取阶段的清单/TSV 缓冲此刻已不可达，但 JSC 不会主动把堆还给操作系统；
  // verify 稍后在同一 2G 任务预算内冲峰值，这里强制回收一次。
  Bun.gc(true);
  const code = await finishVerifyStep(cfg, running, started, tsv);
  console.log(`total_ms=${Math.round(performance.now() - total)}`);
  return code;
}

process.exit(await main(process.argv.slice(2)));
