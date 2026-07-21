"""生成最小 OOXML fixture（docx/pptx/xlsx）含 docProps/core.xml dcterms 字段。

产物：
- `tests/data/sample-docx-dated.docx`：含 dcterms:created/modified。
- `tests/data/sample-pptx-dated.pptx`：同上。
- `tests/data/sample-xlsx-dated.xlsx`：同上。
- `tests/data/sample-docx-no-core.docx`：缺 docProps/core.xml → 返 (0, 0)。

OOXML 是 zip 容器，含 `docProps/core.xml`（XML，含 dcterms:created 等）。
infer crate 识别 zip + `[Content_Types].xml` 子文件 → 不同 OOXML MIME。
本 fixture 简化：只放 `[Content_Types].xml`（指明文档类型）+ `docProps/core.xml`。
"""

from __future__ import annotations

import sys
import zipfile
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"

CORE_XML_DATED = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <dcterms:created xsi:type="dcterms:W3CDTF">2017-02-14T10:30:00Z</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">2018-01-01T12:00:00Z</dcterms:modified>
</cp:coreProperties>
"""

CONTENT_TYPES_DOCX = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>
"""

CONTENT_TYPES_PPTX = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>
"""

CONTENT_TYPES_XLSX = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>
"""


DOCX_DOCUMENT_XML = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>增值税发票 报销单据 开票日期</w:t></w:r></w:p></w:body>
</w:document>
"""

PPTX_SLIDE1_XML = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>项目进展工作报告 汇报总结</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
</p:sld>
"""

XLSX_SHARED_STRINGS_XML = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
  <si><t>合同条款</t></si><si><t>甲方乙方</t></si>
</sst>
"""

# 各格式正文 entry：供 copy-doc 内容分类的 extract_text 提取（时间解析不读它们）。
BODY_ENTRIES = {
    "docx": [("word/document.xml", DOCX_DOCUMENT_XML)],
    "pptx": [("ppt/slides/slide1.xml", PPTX_SLIDE1_XML)],
    "xlsx": [("xl/sharedStrings.xml", XLSX_SHARED_STRINGS_XML)],
}


def write_ooxml(
    path: Path, content_types: str, with_core: bool = True, body: str | None = None
) -> None:
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", content_types)
        if with_core:
            z.writestr("docProps/core.xml", CORE_XML_DATED)
        for name, content in BODY_ENTRIES.get(body or "", []):
            z.writestr(name, content)


def main() -> None:
    sys.stdout.reconfigure(newline="\n")
    DATA_DIR.mkdir(parents=True, exist_ok=True)

    write_ooxml(DATA_DIR / "sample-docx-dated.docx", CONTENT_TYPES_DOCX, body="docx")
    write_ooxml(DATA_DIR / "sample-pptx-dated.pptx", CONTENT_TYPES_PPTX, body="pptx")
    write_ooxml(DATA_DIR / "sample-xlsx-dated.xlsx", CONTENT_TYPES_XLSX, body="xlsx")
    write_ooxml(DATA_DIR / "sample-docx-no-core.docx", CONTENT_TYPES_DOCX, with_core=False)

    for name in [
        "sample-docx-dated.docx",
        "sample-pptx-dated.pptx",
        "sample-xlsx-dated.xlsx",
        "sample-docx-no-core.docx",
    ]:
        print(f"wrote {DATA_DIR / name}")


if __name__ == "__main__":
    main()
