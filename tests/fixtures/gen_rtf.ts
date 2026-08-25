// 生成最小 RTF fixture 含 \creatim/\revtim 时间组 + 正文。
//
// 产物：sample-rtf-dated.rtf（\creatim 2017-02-14 → 桶 2017/02）。RTF 纯 ASCII
// 文本；中文正文用 \uN? 转义（供 copy-doc 内容分类提取）。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DATA_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "data");

function uEscape(text: string): string {
  let out = "";
  for (const ch of text) {
    out += ch.charCodeAt(0) > 127 ? `\\u${ch.charCodeAt(0)}?` : ch;
  }
  return out;
}

const body = uEscape("增值税发票 报销单据 开票日期");
const rtf =
  "{\\rtf1\\ansi" +
  "{\\info" +
  "{\\creatim\\yr2017\\mo2\\dy14\\hr10\\min30\\sec0}" +
  "{\\revtim\\yr2018\\mo1\\dy1\\hr12\\min0\\sec0}" +
  "}" +
  `\\pard ${body}\\par` +
  "}";

const out = join(DATA_DIR, "sample-rtf-dated.rtf");
mkdirSync(DATA_DIR, { recursive: true });
writeFileSync(out, Buffer.from(rtf, "ascii"));
console.log(`wrote ${out}`);
