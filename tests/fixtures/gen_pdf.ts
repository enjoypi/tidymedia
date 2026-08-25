// 生成最小 PDF fixture（含 `/Info /CreationDate` + `/ModDate` 字面量）。
//
// 产物：sample-pdf-dated.pdf（两时间字段）+ sample-pdf-no-info.pdf（缺 /Info dict）。
// PDF 时间格式 `D:YYYYMMDDHHmmSSOHH'mm'`。CreationDate=2017-02-14 10:30:00Z
// （epoch 1487068200），ModDate=2018-01-01 12:00:00Z。跳过完整 xref/trailer
// （扫描器只看字面量），保留 `%%EOF`。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DATA_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "data");

const dated = Buffer.from(
  "%PDF-1.4\n" +
    "1 0 obj\n" +
    "<< /CreationDate (D:20170214103000Z) /ModDate (D:20180101120000Z) >>\n" +
    "endobj\n" +
    "trailer << /Info 1 0 R >>\n" +
    "%%EOF\n",
);
writeFileSync(join(DATA_DIR, "sample-pdf-dated.pdf"), dated);

const noInfo = Buffer.from(
  "%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer << >>\n%%EOF\n",
);
writeFileSync(join(DATA_DIR, "sample-pdf-no-info.pdf"), noInfo);

mkdirSync(DATA_DIR, { recursive: true });
console.log(`wrote ${join(DATA_DIR, "sample-pdf-dated.pdf")}`);
console.log(`wrote ${join(DATA_DIR, "sample-pdf-no-info.pdf")}`);
