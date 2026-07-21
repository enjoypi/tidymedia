r"""生成最小 RTF fixture 含 \creatim/\revtim 时间组 + 正文。

产物：
- `tests/data/sample-rtf-dated.rtf`：\creatim 2017-02-14 → 桶 2017/02。

RTF 纯 ASCII 文本格式；中文正文用 \uN? 转义（供 copy-doc 内容分类提取）。
"""

from __future__ import annotations

import sys
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"


def u_escape(text: str) -> str:
    return "".join(f"\\u{ord(c)}?" if ord(c) > 127 else c for c in text)


def main() -> None:
    sys.stdout.reconfigure(newline="\n")
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    body = u_escape("增值税发票 报销单据 开票日期")
    rtf = (
        "{\\rtf1\\ansi"
        "{\\info"
        "{\\creatim\\yr2017\\mo2\\dy14\\hr10\\min30\\sec0}"
        "{\\revtim\\yr2018\\mo1\\dy1\\hr12\\min0\\sec0}"
        "}"
        f"\\pard {body}\\par"
        "}"
    )
    out = DATA_DIR / "sample-rtf-dated.rtf"
    out.write_bytes(rtf.encode("ascii"))
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
