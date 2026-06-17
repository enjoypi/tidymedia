"""生成最小 PDF fixture（含 `/Info /CreationDate` + `/ModDate` 字面量）。

产物：
- `tests/data/sample-pdf-dated.pdf`：含两个时间字段，扫描器命中后 doc_created/doc_modified 都非零。
- `tests/data/sample-pdf-no-info.pdf`：缺 /Info dict，扫描器找不到 key → 返 (0, 0)。

PDF 时间格式（ISO 32000-1 § 7.9.4）：`D:YYYYMMDDHHmmSSOHH'mm'`，O 为 `+`/`-`/`Z`。
本 fixture 用：
- CreationDate=2017-02-14 10:30:00Z（UTC epoch 1487068200）
- ModDate=2018-01-01 12:00:00Z（UTC epoch 1514808000）

PDF 头部 `%PDF-1.4` magic bytes 让 infer crate 识别为 application/pdf；本 fixture
跳过完整 xref/trailer 结构（扫描器只看字面量），但保留 `%%EOF` 让有些工具不投诉。
"""

from __future__ import annotations

import sys
from pathlib import Path

# 项目根：tests/fixtures/../data
DATA_DIR = Path(__file__).resolve().parent.parent / "data"


def main() -> None:
    sys.stdout.reconfigure(newline="\n")
    DATA_DIR.mkdir(parents=True, exist_ok=True)

    # Happy path：含 /CreationDate + /ModDate。
    dated = (
        b"%PDF-1.4\n"
        b"1 0 obj\n"
        b"<< /CreationDate (D:20170214103000Z) /ModDate (D:20180101120000Z) >>\n"
        b"endobj\n"
        b"trailer << /Info 1 0 R >>\n"
        b"%%EOF\n"
    )
    (DATA_DIR / "sample-pdf-dated.pdf").write_bytes(dated)

    # 损坏：合法 PDF 头但无 /Info dict → 扫描器返 (0, 0)，create_time 退到 P4 mtime。
    no_info = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer << >>\n%%EOF\n"
    (DATA_DIR / "sample-pdf-no-info.pdf").write_bytes(no_info)

    print(f"wrote {DATA_DIR / 'sample-pdf-dated.pdf'}")
    print(f"wrote {DATA_DIR / 'sample-pdf-no-info.pdf'}")


if __name__ == "__main__":
    main()
