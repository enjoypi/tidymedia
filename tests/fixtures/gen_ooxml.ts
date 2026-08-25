// 生成最小 OOXML fixture（docx/pptx/xlsx）含 docProps/core.xml dcterms 字段。
//
// 产物：sample-docx-dated.docx / sample-pptx-dated.pptx / sample-xlsx-dated.xlsx
// （dcterms:created=2017-02-14 → 桶 2017/02）+ sample-docx-no-core.docx（缺 core.xml）。
// OOXML 是 zip 容器，简化只放 `[Content_Types].xml` + `docProps/core.xml`。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildZip, type ZipEntry } from "./lib/zip.ts";

const DATA_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "data");

const CORE_XML_DATED = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <dcterms:created xsi:type="dcterms:W3CDTF">2017-02-14T10:30:00Z</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">2018-01-01T12:00:00Z</dcterms:modified>
</cp:coreProperties>
`;

const CONTENT_TYPES_DOCX = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>
`;

const CONTENT_TYPES_PPTX = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>
`;

const CONTENT_TYPES_XLSX = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>
`;

const DOCX_DOCUMENT_XML = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>增值税发票 报销单据 开票日期</w:t></w:r></w:p></w:body>
</w:document>
`;

const PPTX_SLIDE1_XML = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>项目进展工作报告 汇报总结</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
</p:sld>
`;

const XLSX_SHARED_STRINGS_XML = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
  <si><t>合同条款</t></si><si><t>甲方乙方</t></si>
</sst>
`;

// 各格式正文 entry：供 copy-doc 内容分类的 extract_text 提取（时间解析不读它们）。
const BODY_ENTRIES: Record<string, [string, string][]> = {
  docx: [["word/document.xml", DOCX_DOCUMENT_XML]],
  pptx: [["ppt/slides/slide1.xml", PPTX_SLIDE1_XML]],
  xlsx: [["xl/sharedStrings.xml", XLSX_SHARED_STRINGS_XML]],
};

function writeOoxml(
  name: string,
  contentTypes: string,
  withCore: boolean,
  body?: string,
): void {
  const entries: ZipEntry[] = [
    { name: "[Content_Types].xml", data: Buffer.from(contentTypes) },
  ];
  if (withCore) {
    entries.push({ name: "docProps/core.xml", data: Buffer.from(CORE_XML_DATED) });
  }
  for (const [entryName, content] of BODY_ENTRIES[body ?? ""] ?? []) {
    entries.push({ name: entryName, data: Buffer.from(content) });
  }
  const zip = buildZip(entries);
  writeFileSync(join(DATA_DIR, name), zip);
  console.log(`wrote ${join(DATA_DIR, name)}`);
}

mkdirSync(DATA_DIR, { recursive: true });
writeOoxml("sample-docx-dated.docx", CONTENT_TYPES_DOCX, true, "docx");
writeOoxml("sample-pptx-dated.pptx", CONTENT_TYPES_PPTX, true, "pptx");
writeOoxml("sample-xlsx-dated.xlsx", CONTENT_TYPES_XLSX, true, "xlsx");
writeOoxml("sample-docx-no-core.docx", CONTENT_TYPES_DOCX, false);
