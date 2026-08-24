// Step 4：解析 `tidymedia verify --report` 的 VerifyReport JSON，输出对账汇总：
// MISMATCH（桶对账）/ DIFFER（文件名时间冲突）/ duplicate_verdict 分布 /
// pattern 计数。替代旧 compare_buckets.py + filename_conflict.py + 45_check_copied.py。
// Usage: bun analyze_verify.ts <verify.json>
/// <reference path="./lib/bun.d.ts" />

import { analyzeVerifyReport } from "./lib/verify_report.ts";

async function main(args: string[]): Promise<number> {
  const jsonPath = args[0];
  if (!jsonPath) {
    console.error("Usage: bun analyze_verify.ts <verify.json>");
    return 2;
  }
  const json = JSON.parse(await Bun.file(jsonPath).text());
  const s = analyzeVerifyReport(json);

  console.log("---summary---");
  console.log(`scanned=${s.scanned}`);
  console.log(`compared=${s.compared}`);
  console.log(`mismatched=${s.mismatched}`);
  console.log(`decision_failed=${s.decision_failed}`);
  console.log(`with_name_time=${s.with_name_time}`);

  console.log(`MISMATCH_count=${s.mismatchRows.length}`);
  if (s.mismatchRows.length > 0) {
    console.log("---mismatch by from---");
    const byFrom = new Map<string, number>();
    for (const m of s.mismatchRows) {
      byFrom.set(m.from, (byFrom.get(m.from) ?? 0) + 1);
    }
    for (const [k, n] of [...byFrom.entries()].sort()) {
      console.log(`${n}\t${k}`);
    }
    console.log("---MISMATCH details---");
    for (const m of s.mismatchRows) {
      console.log(
        `MISMATCH\texp=${m.exp}\ttgt=${m.tgt}\tfrom=${m.from}\tmake=${m.make}\tmodel=${m.model}\t${m.source}`,
      );
    }
  }

  console.log(`DIFFER_count=${s.differRows.length}`);
  if (s.differRows.length > 0) {
    console.log("---DIFFER details---");
    for (const d of s.differRows) {
      console.log(`DIFFER\tname=${d.name}\ttgt=${d.tgt}\t${d.source}`);
    }
  }

  console.log("---duplicate_verdict---");
  for (const [k, n] of Object.entries(s.verdictCounts).sort((a, b) => b[1] - a[1])) {
    console.log(`${n}\t${k}`);
  }

  if (Object.keys(s.patternCounts).length > 0) {
    console.log("---patterns---");
    for (const [k, n] of Object.entries(s.patternCounts).sort((a, b) => b[1] - a[1])) {
      console.log(`${n}\t${k}`);
    }
  }
  return 0;
}

const code = await main(process.argv.slice(2));
process.exit(code);
