"""生成最小 ODF fixture（odt/ods/odp）含 meta.xml 创建/修改时间 + content.xml 正文。

产物：
- `tests/data/sample-odt-dated.odt`：meta:creation-date=2017-02-14 → 桶 2017/02。
- `tests/data/sample-ods-dated.ods` / `sample-odp-dated.odp`：同上。

ODF 是 zip 容器：`mimetype`（stored 首 entry）+ `meta.xml`（时间）+
`content.xml`（正文，供 copy-doc 内容分类提取）。infer 无 ODF matcher，
运行时靠扩展名 fallback 命中 office 路由。
"""

from __future__ import annotations

import sys
import zipfile
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"

META_XML = """<?xml version="1.0" encoding="UTF-8"?>
<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <office:meta>
    <meta:creation-date>2017-02-14T10:30:00Z</meta:creation-date>
    <dc:date>2018-01-01T12:00:00Z</dc:date>
  </office:meta>
</office:document-meta>
"""

CONTENT_XML = """<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
  <office:body><office:text>
    <text:p>本合同由甲方与乙方签订，双方约定服务条款如下。</text:p>
  </office:text></office:body>
</office:document-content>
"""

MIMETYPES = {
    "odt": "application/vnd.oasis.opendocument.text",
    "ods": "application/vnd.oasis.opendocument.spreadsheet",
    "odp": "application/vnd.oasis.opendocument.presentation",
}


def write_odf(path: Path, mimetype: str) -> None:
    with zipfile.ZipFile(path, "w") as z:
        # ODF spec：mimetype 必须是首 entry 且 stored（不压缩）。
        z.writestr(
            zipfile.ZipInfo("mimetype"), mimetype, compress_type=zipfile.ZIP_STORED
        )
        z.writestr("meta.xml", META_XML, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("content.xml", CONTENT_XML, compress_type=zipfile.ZIP_DEFLATED)


def main() -> None:
    sys.stdout.reconfigure(newline="\n")
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    for ext, mimetype in MIMETYPES.items():
        out = DATA_DIR / f"sample-{ext}-dated.{ext}"
        write_odf(out, mimetype)
        print(f"wrote {out}")


if __name__ == "__main__":
    main()
