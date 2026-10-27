/// <reference path="./lib/bun.d.ts" />

import { mkdirSync, writeFileSync } from "node:fs";
import { freemem } from "node:os";
import { dirname } from "node:path";
import { exiftoolBin, loadConfig, tidymediaBin, workPath } from "./lib/config.ts";
import type { SkillConfig } from "./lib/config.ts";
import { extractExif, listDirCounts, spawnToFiles } from "./lib/extract.ts";
import { isImageRunning, killTree, pickConcurrency, reapStale, recordPid } from "./lib/procs.ts";
import { startVerify, verifyCommand, verifyExitCode } from "./lib/verify.ts";
import type { RunningVerify } from "./lib/verify.ts";

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
  const counts = await listDirCounts(src);
  const concurrency = pickConcurrency(cfg.exiftoolConcurrency, freemem(), cfg.exiftoolMemMb * 1024 * 1024);
  const tsv = workPath(cfg, cfg.exifTsv);
  const r = await extractExif({
    bin, fmt: cfg.exiftoolTsvP, counts, work: cfg.workDir, concurrency, run: spawnToFiles,
    tsv, err: `${tsv}.err`, onSpawn: (pid) => recordPid(pids, pid),
  });
  writeFileSync(pids, "");
  console.log(`exif_rows=${r.rows} exif_groups=${r.groups} exif_exit=${r.exit} extract_ms=${Math.round(performance.now() - t0)}`);
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
