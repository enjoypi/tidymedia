// 生成最小 ODF fixture（odt/ods/odp）含 meta.xml 创建/修改时间 + content.xml 正文。
//
// 产物：sample-odt-dated.odt / sample-ods-dated.ods / sample-odp-dated.odp
// （meta:creation-date=2017-02-14 → 桶 2017/02）。ODF 是 zip 容器：mimetype
// stored 首 entry + meta.xml（时间）+ content.xml（正文）。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildZip, type ZipEntry } from "./lib/zip.ts";

const DATA_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "data");

const META_XML = `<?xml version="1.0" encoding="UTF-8"?>
<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <office:meta>
    <meta:creation-date>2017-02-14T10:30:00Z</meta:creation-date>
    <dc:date>2018-01-01T12:00:00Z</dc:date>
  </office:meta>
</office:document-meta>
`;

const CONTENT_XML = `<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
  <office:body><office:text>
    <text:p>本合同由甲方与乙方签订，双方约定服务条款如下。</text:p>
  </office:text></office:body>
</office:document-content>
`;

const MIMETYPES: Record<string, string> = {
  odt: "application/vnd.oasis.opendocument.text",
  ods: "application/vnd.oasis.opendocument.spreadsheet",
  odp: "application/vnd.oasis.opendocument.presentation",
};

function writeOdf(ext: string, mimetype: string): void {
  const entries: ZipEntry[] = [
    { name: "mimetype", data: Buffer.from(mimetype), stored: true },
    { name: "meta.xml", data: Buffer.from(META_XML) },
    { name: "content.xml", data: Buffer.from(CONTENT_XML) },
  ];
  const zip = buildZip(entries);
  const out = join(DATA_DIR, `sample-${ext}-dated.${ext}`);
  writeFileSync(out, zip);
  console.log(`wrote ${out}`);
}

mkdirSync(DATA_DIR, { recursive: true });
for (const [ext, mimetype] of Object.entries(MIMETYPES)) {
  writeOdf(ext, mimetype);
}
