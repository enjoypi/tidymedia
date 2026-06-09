r"""tidy-verify Step 4：从 source 路径里抽显式时间字符串，与 target 桶比对。

Usage:
    SOURCE_ROOT='D:\Users\Public\Pictures\2006\' uv run filename_conflict.py <copy_lines.log>

SOURCE_ROOT MUST 以路径分隔符结尾，否则源根目录里的年份段会污染 parse_path。

抽取模式（按优先级）：
  1. YYYY[-_./ ]?M[M]      YYYY ∈ 1995..2030, M 1..12（兼容单数字月份如 2008-6）
  2. YYYYMMDD              8 连续数字
  3. YY-MM-DD              YY<50 → 20YY, ≥50 → 19YY（含 DD 减少假阳）

月份单数字兼容 WHY：`西宁 2008-6-19 13-08-21.jpg` 这类「日期 HH-MM-SS」文件名，
若 regex 1 不接受单数字月（M=6）会回退到 regex 3 把 HH-MM-SS（13-08-21）当 YY-MM-DD
误判为 2013:08，产生 DIFFER 假阳。

YYYY 前非数字边界 WHY：相机命名 `P1120296.JPG` 子串 `2029` 落在 1995-2030 范围会
被 regex 1 误识别为 2029:06；前缀加 `(^|[^0-9])` 要求 YYYY 前是字符串首或非数字字符。
中文目录段（如 `荣县200607`）前是 UTF-8 末字节非数字，仍能命中。

输出：
  每条 DIFFER 一行：`DIFFER \t name=YYYY:MM \t tgt=YYYY:MM \t <source>`
  末尾 with_name_time=<路径含可识别时间的总数>
"""
import os
import re
import sys

# Windows 上 Python stdout 默认 CRLF；强制 LF 与既有下游脚本（grep/awk/diff）口径一致。
sys.stdout.reconfigure(newline="\n")

SEP = chr(92)  # Windows path separator

# Python `re` alternation 是 leftmost-first（不像 POSIX 的 leftmost-longest）：
# `(0?[1-9]|1[012])` 在 `2008-10-15` 上会抢先吃 `2008-1` 而非 `2008-10`。
# 必须让长 token (`1[012]`) 优先，再回落短 token (`0?[1-9]`)。
RE_YEAR_MONTH = re.compile(
    r"(?:^|[^0-9])(199[5-9]|20[0-2][0-9]|2030)[-_./ ]?(1[012]|0?[1-9])"
)
RE_YYYYMMDD = re.compile(
    r"(?:^|[^0-9])(199[5-9]|20[0-2][0-9]|2030)(0[1-9]|1[012])(0[1-9]|[12][0-9]|3[01])"
)
RE_YY_MM_DD = re.compile(
    r"(?:^|[^0-9])([0-9][0-9])-(0[1-9]|1[012])-(0[1-9]|[12][0-9]|3[01])(?:[^0-9]|$)"
)


def parse_path(s):
    """返回 'YYYY:MM' 或 ''。"""
    m = RE_YEAR_MONTH.search(s)
    if m:
        y, mo = m.group(1), m.group(2)
        if len(mo) == 1:
            mo = "0" + mo
        return f"{y}:{mo}"
    m = RE_YYYYMMDD.search(s)
    if m:
        return f"{m.group(1)}:{m.group(2)}"
    m = RE_YY_MM_DD.search(s)
    if m:
        y = int(m.group(1))
        y += 2000 if y < 50 else 1900
        return f"{y:04d}:{m.group(2)}"
    return ""


def extract_target_bucket(target):
    parts = target.split(SEP)
    for k in range(len(parts) - 2):
        y, mo = parts[k], parts[k + 1]
        if len(y) == 4 and len(mo) == 2 and y.isdigit() and mo.isdigit():
            return f"{y}:{mo}"
    return "NO_BUCKET"


def main():
    source_root = os.environ.get("SOURCE_ROOT", "")
    if not source_root:
        print(
            "ERROR: set SOURCE_ROOT env var (with trailing path separator)",
            file=sys.stderr,
        )
        sys.exit(2)
    if len(sys.argv) != 2:
        print("Usage: filename_conflict.py <copy_lines.log>", file=sys.stderr)
        sys.exit(2)

    counted = 0
    out = []
    with open(sys.argv[1], encoding="utf-8") as f:
        for line in f:
            si = line.find("source=")
            ti = line.find(" target=")
            if si < 0 or ti < 0:
                continue
            s = line[si + 7 : ti]
            t = line[ti + 8 :].rstrip("\r\n")

            tgt = extract_target_bucket(t)
            rel = s[len(source_root):] if s.startswith(source_root) else s
            nt = parse_path(rel)
            if not nt:
                continue
            if nt != tgt:
                out.append(f"DIFFER\tname={nt}\ttgt={tgt}\t{s}")
            counted += 1

    if out:
        print("\n".join(out))
    print("---")
    print(f"with_name_time={counted}")


if __name__ == "__main__":
    main()
