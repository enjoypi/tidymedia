"""tidy-verify Step 3：EXIF 年月 vs tidymedia 推断的 target 桶 YYYY/MM。

Usage:
    uv run compare_buckets.py <exif.tsv> <copy_lines.log> [tz_hours=8]

输入（顺序固定）：
  1. exif.tsv          每行 8 列（与 02_extract_exif.sh 同步维护）：
                       path \t DTO \t QT:CreationDate \t QT:CreateDate \t CreateDate \t FileModifyDate \t Make \t Model
                       空字段以 `-` 输出（exiftool -T 默认）
  2. copy_lines.log    tidymedia --log-level=debug move --dry-run 抽出的
                       `operation="copy_file"` 行，含 `source=` / `target=`

输出：
  每条 MISMATCH 一行：
    `MISMATCH \t exp=YYYY:MM \t tgt=YYYY:MM \t from=DTO|QTCreationDate|QTCreateDate|CreateDate|FsMtime|NONE|MISSING \t make=<Make> \t model=<Model> \t <source>`
  末尾 compared=<对比的 copy_file 行数>

  from 优先级与 tidymedia P0..P4 对齐：DTO > QTCreationDate > QTCreateDate > CreateDate > FsMtime；
  exiftool 拿到容器时间而 tidymedia 走 FsMtime 兜底即可检出（如 pnot 老 QuickTime nom-exif 漏读）。
"""
import re
import sys

# Windows 上 Python stdout 默认 CRLF；强制 LF 与既有下游脚本（grep/awk/diff）口径一致。
sys.stdout.reconfigure(newline="\n")

SEP = chr(92)  # Windows path separator; avoid Write tool eating \

FROM_LABEL = ["DTO", "QTCreationDate", "QTCreateDate", "CreateDate", "FsMtime"]

# QuickTime 系字段（QT:CreationDate / QT:CreateDate）是 UTC 语义：nom-exif 合成带时区
# DateTime、tidymedia 走 dt.timestamp() 取 UTC epoch 再 .to_offset(+tz) 归档，月末/午夜会
# 跨月。DTO/CreateDate(EXIF naive) 是相机本地、+offset 解释又 +offset 归档首尾抵消，前 7 字
# 符即桶。FsMtime exiftool 输出带本地后缀，前 7 字符即本地年月。enumerate(start=1) 下
# FROM_LABEL 的 QTCreationDate=i2、QTCreateDate=i3，故仅 idx 2/3 需时区转换。
QT_COL_INDICES = (2, 3)

# exiftool 时间串：YYYY:MM:DD HH:MM:SS 可选时区后缀 ±HH:MM 或 Z。
_QT_TIME_RE = re.compile(
    r"(\d{4}):(\d\d):(\d\d) (\d\d):(\d\d):(\d\d)(?:([+-])(\d\d):(\d\d)|Z)?"
)


def _qt_bucket(v, tz_hours):
    r"""把 QuickTime 时间串按 UTC→配置时区转换后取 'YYYY:MM'；解析失败回退前 7 字符。

    带时区后缀者按其 offset 归一化到 UTC、无后缀者当 UTC，再 +tz_hours 取年月。
    本地时刻 = naive + (tz_hours*60 - src_offset)，timedelta 自动处理跨日/月进位。
    """
    import datetime

    m = _QT_TIME_RE.match(v)
    if not m:
        return v[:7]
    year, mon, day, hour, minute, sec = (int(m.group(i)) for i in range(1, 7))
    src_off_min = 0
    if m.group(7):  # 带 ±HH:MM 后缀
        sign = 1 if m.group(7) == "+" else -1
        src_off_min = sign * (int(m.group(8)) * 60 + int(m.group(9)))
    naive = datetime.datetime(year, mon, day, hour, minute, sec)
    local = naive + datetime.timedelta(minutes=tz_hours * 60 - src_off_min)
    return f"{local.year:04d}:{local.month:02d}"


def parse_exif_tsv(path, tz_hours=8):
    """Pass 1：建 expected/from/make/model map（key = 反斜杠路径）。"""
    expected, src_from, make, model = {}, {}, {}, {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            # exiftool 在 Windows 用 CRLF 行尾，末字段会带 \r 污染输出
            row = line.rstrip("\r\n").split("\t")
            if len(row) < 8:
                continue
            # 末字段（Model）单独再剥 \r（rstrip 已处理行尾，这里防中间字段被 split 后仍有残留）
            row[-1] = row[-1].rstrip("\r")
            p = row[0].replace("/", SEP)
            pick, label = "", "NONE"
            # 索引对齐 col 2..6（python 0-based = idx 1..5）
            for i, lab in enumerate(FROM_LABEL, start=1):
                v = row[i]
                # exiftool 时间格式 YYYY:MM:DD HH:MM:SS，前 7 字符即 YYYY:MM
                if len(v) >= 7 and v[4] == ":":
                    pick = _qt_bucket(v, tz_hours) if i in QT_COL_INDICES else v[:7]
                    label = lab
                    break
            expected[p] = pick if pick else "NONE"
            src_from[p] = label
            mk = row[6]
            md = row[7]
            make[p] = "-" if mk in ("", "-") else mk
            model[p] = "-" if md in ("", "-") else md
    return expected, src_from, make, model


def extract_target_bucket(target):
    r"""从 target 路径里抽第一个 \YYYY\MM\ 段作为桶（返回 'YYYY:MM' 或 'NO_BUCKET'）。"""
    parts = target.split(SEP)
    for k in range(len(parts) - 2):
        y, m = parts[k], parts[k + 1]
        if len(y) == 4 and len(m) == 2 and y.isdigit() and m.isdigit():
            return f"{y}:{m}"
    return "NO_BUCKET"


def main():
    if len(sys.argv) not in (3, 4):
        print(
            "Usage: compare_buckets.py <exif.tsv> <copy_lines.log> [tz_hours=8]",
            file=sys.stderr,
        )
        sys.exit(2)
    tz_hours = int(sys.argv[3]) if len(sys.argv) == 4 else 8
    expected, src_from, make, model = parse_exif_tsv(sys.argv[1], tz_hours)

    out, total = [], 0
    with open(sys.argv[2], encoding="utf-8") as f:
        for line in f:
            si = line.find("source=")
            ti = line.find(" target=")
            if si < 0 or ti < 0:
                continue
            s = line[si + 7 : ti]
            t = line[ti + 8 :].rstrip("\r\n")
            tgt = extract_target_bucket(t)
            ex = expected.get(s, "MISSING")
            lab = src_from.get(s, "MISSING")
            mk = make.get(s, "-")
            md = model.get(s, "-")
            if ex != tgt:
                out.append(
                    f"MISMATCH\texp={ex}\ttgt={tgt}\tfrom={lab}\tmake={mk}\tmodel={md}\t{s}"
                )
            total += 1

    if out:
        print("\n".join(out))
    print("---")
    print(f"compared={total}")


if __name__ == "__main__":
    main()
