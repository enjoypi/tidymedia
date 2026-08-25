// 生成最小 EPUB fixture 含 OPF dc:date/dcterms:modified + xhtml 章节正文。
//
// 产物：sample-epub-dated.epub（dc:date=2017-02-14 → 桶 2017/02）。EPUB 是 zip
// 容器：mimetype stored 首 entry + META-INF/container.xml（双跳到 content.opf）
// + OEBPS/content.opf（时间）+ OEBPS/ch1.xhtml（正文）。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildZip, type ZipEntry } from "./lib/zip.ts";

const DATA_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "data");

const CONTAINER_XML = `<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
`;

const CONTENT_OPF = `<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>sample</dc:title>
    <dc:date>2017-02-14T10:30:00Z</dc:date>
    <meta property="dcterms:modified">2018-01-01T12:00:00Z</meta>
  </metadata>
</package>
`;

const CH1_XHTML = `<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <body><p>项目进展工作报告：本季度里程碑与交付总结。</p></body>
</html>
`;

const entries: ZipEntry[] = [
  { name: "mimetype", data: Buffer.from("application/epub+zip"), stored: true },
  { name: "META-INF/container.xml", data: Buffer.from(CONTAINER_XML) },
  { name: "OEBPS/content.opf", data: Buffer.from(CONTENT_OPF) },
  { name: "OEBPS/ch1.xhtml", data: Buffer.from(CH1_XHTML) },
];
const zip = buildZip(entries);
const out = join(DATA_DIR, "sample-epub-dated.epub");
mkdirSync(DATA_DIR, { recursive: true });
writeFileSync(out, zip);
console.log(`wrote ${out}`);
