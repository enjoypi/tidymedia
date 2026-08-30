// Step 5 前半：对 verify.json 的候选集 U（MISMATCH ∪ DIFFER）逐文件跑 exiftool
// 全量时间 dump，组装证据卡片 markdown（除「推荐」外全填），AI 只研判+提问。
// Usage: bun collect_evidence.ts <verify.json> <src_root> [out.md]
/// <reference path="./lib/bun.d.ts" />

import { exiftoolBin, loadConfig, workPath } from "./lib/config.ts";
import {
  buildCard,
  parseExiftoolDump,
  relativePath,
  selectReviewEntries,
  type EvidenceEntry,
} from "./lib/evidence.ts";
import { decodeExifText } from "./lib/gbk.ts";

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

  // 逐文件全量 dump：-s 短名 -G 分组 -time:all 全部时间类 tag；多文件一次
  // spawn 会让 -G 输出跨文件交错难解析，U 通常很小，逐文件调用换取解析简单。
  const cards: string[] = [];
  for (const e of review) {
    const proc = Bun.spawn([bin, "-s", "-G", "-time:all", "-Make", "-Model", e.source_path]);
    const out = new Uint8Array(await new Response(proc.stdout).arrayBuffer());
    await proc.exited;
    const dump = parseExiftoolDump(decodeExifText(out).replace(/\r\n/g, "\n"));
    cards.push(buildCard(e, dump, relativePath(e.source_path, srcRoot)));
  }

  const outPath = outArg ?? workPath(cfg, "evidence.md");
  await Bun.write(outPath, cards.join("\n"));
  console.log(`review_count=${review.length}`);
  console.log(`evidence=${outPath}`);
  return 0;
}

const code = await main(process.argv.slice(2));
process.exit(code);
