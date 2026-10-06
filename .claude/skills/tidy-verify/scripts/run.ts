/// <reference path="./lib/bun.d.ts" />

import { mkdirSync } from "node:fs";
import { exiftoolBin, loadConfig, tidymediaBin, workPath } from "./lib/config.ts";
import type { SkillConfig } from "./lib/config.ts";
import { extractExif, listDirCounts, spawnRunner } from "./lib/extract.ts";
import { verifyCommand } from "./lib/verify.ts";

async function extractStep(cfg: SkillConfig, src: string): Promise<string | null> {
  const bin = await exiftoolBin(cfg);
  if (!bin) {
    console.log("exiftool_missing=1（跳过 --exif-tsv，verify 内部判定仍有效）");
    return null;
  }
  const t0 = performance.now();
  const counts = await listDirCounts(src);
  const r = await extractExif({
    bin, fmt: cfg.exiftoolTsvP, counts, work: cfg.workDir, concurrency: cfg.exiftoolConcurrency, run: spawnRunner,
  });
  const tsv = workPath(cfg, cfg.exifTsv);
  await Bun.write(tsv, r.tsv);
  await Bun.write(`${tsv}.err`, r.err);
  const rows = r.tsv === "" ? 0 : r.tsv.split("\n").length - 1;
  console.log(`exif_rows=${rows} exif_groups=${r.groups} exif_exit=${r.exit} extract_ms=${Math.round(performance.now() - t0)}`);
  return tsv;
}

async function verifyStep(cfg: SkillConfig, bin: string, src: string, out: string, tsv: string | null): Promise<number> {
  const t0 = performance.now();
  const r = await spawnRunner(verifyCommand(bin, src, out, tsv, workPath(cfg, cfg.verifyReport)));
  const summary = new TextDecoder().decode(r.out);
  await Bun.write(workPath(cfg, "summary.txt"), summary);
  await Bun.write(workPath(cfg, "verify.err"), r.err);
  console.log(summary.trimEnd());
  console.log(`verify_exit=${r.exit} verify_ms=${Math.round(performance.now() - t0)}`);
  return r.exit;
}

async function main(args: string[]): Promise<number> {
  const [src, out, work] = args;
  if (!src || !out || !work) {
    console.error("Usage: bun run.ts <source_dir> <output_dir> <work_dir>");
    return 2;
  }
  const cfg = { ...(await loadConfig()), workDir: work };
  const bin = await tidymediaBin(cfg);
  if (!bin) {
    console.error(`tidymedia 缺失：${cfg.tidymediaBin}，先 just build`);
    return 2;
  }
  mkdirSync(work, { recursive: true });
  console.log(`work_dir=${work}`);
  const tsv = await extractStep(cfg, src);
  return verifyStep(cfg, bin, src, out, tsv);
}

process.exit(await main(process.argv.slice(2)));
