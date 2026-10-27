// Step 3 前半：对 verify.json 的候选集 U（MISMATCH ∪ DIFFER）逐文件跑 exiftool
// 全量时间 dump，组装证据卡片 markdown（除「推荐」外全填），AI 只研判+提问。
// Usage: bun collect_evidence.ts <verify.json> <src_root> [out.md]
/// <reference path="./lib/bun.d.ts" />

import { freemem } from "node:os";
import { exiftoolBin, loadConfig, workPath } from "./lib/config.ts";
import {
  collectCards,
  selectReviewEntries,
  type EvidenceEntry,
} from "./lib/evidence.ts";
import { pickConcurrency, spawnRunner } from "./lib/procs.ts";

async function main(args: string[]): Promise<number> {
  const [jsonPath, srcRoot, outArg] = args;
  if (!jsonPath || !srcRoot) {
    console.error("Usage: bun collect_evidence.ts <verify.json> <src_root> [out.md]");
    return 2;
  }
  const cfg = await loadConfig();
  const bin = await exiftoolBin(cfg);
  if (!bin) {
    console.error("exiftool 缺失，无法收集证据（macOS 需自装 exiftool）。");
    return 2;
  }

  const json = JSON.parse(await Bun.file(jsonPath).text()) as {
    entries?: EvidenceEntry[];
  };
  const review = selectReviewEntries(json.entries ?? []);
  if (review.length === 0) {
    console.log("review_count=0（MISMATCH/DIFFER 均为空，无需证据收集）");
    return 0;
  }

  const limit = pickConcurrency(cfg.exiftoolConcurrency, freemem(), cfg.exiftoolMemMb * 1024 * 1024);
  const { cards, failures, warnings } = await collectCards(review, srcRoot, limit, (cmd) => spawnRunner([bin, ...cmd]));
  for (const w of warnings) {
    console.error(`exiftool 警告 path=${w.path} stderr=${w.err}`);
  }
  if (warnings.length > 0) {
    console.error(`evidence_warnings=${warnings.length}`);
  }
  if (failures.length > 0) {
    for (const f of failures) {
      console.error(`exiftool 失败 path=${f.path} exit=${f.exit} stderr=${f.err}`);
    }
    console.error(`evidence_incomplete=${failures.length}，未生成 evidence.md`);
    return 1;
  }

  const outPath = outArg ?? workPath(cfg, "evidence.md");
  await Bun.write(outPath, cards.join("\n"));
  console.log(`review_count=${review.length}`);
  console.log(`evidence=${outPath}`);
  return 0;
}

const code = await main(process.argv.slice(2));
process.exit(code);
