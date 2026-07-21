"""生成最小 EPUB fixture 含 OPF dc:date/dcterms:modified + xhtml 章节正文。

产物：
- `tests/data/sample-epub-dated.epub`：dc:date=2017-02-14 → 桶 2017/02。

EPUB 是 zip 容器：`META-INF/container.xml` → `OEBPS/content.opf`（时间，双跳）
+ `OEBPS/ch1.xhtml`（正文，供 copy-doc 内容分类提取）。
"""

from __future__ import annotations

import sys
import zipfile
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"

CONTAINER_XML = """<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"""

CONTENT_OPF = """<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>sample</dc:title>
    <dc:date>2017-02-14T10:30:00Z</dc:date>
    <meta property="dcterms:modified">2018-01-01T12:00:00Z</meta>
  </metadata>
</package>
"""

CH1_XHTML = """<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <body><p>项目进展工作报告：本季度里程碑与交付总结。</p></body>
</html>
"""


def main() -> None:
    sys.stdout.reconfigure(newline="\n")
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    out = DATA_DIR / "sample-epub-dated.epub"
    with zipfile.ZipFile(out, "w") as z:
        z.writestr(
            zipfile.ZipInfo("mimetype"),
            "application/epub+zip",
            compress_type=zipfile.ZIP_STORED,
        )
        z.writestr(
            "META-INF/container.xml", CONTAINER_XML, compress_type=zipfile.ZIP_DEFLATED
        )
        z.writestr("OEBPS/content.opf", CONTENT_OPF, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/ch1.xhtml", CH1_XHTML, compress_type=zipfile.ZIP_DEFLATED)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
