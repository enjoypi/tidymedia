#!/usr/bin/env bash
# Step 2：exiftool 递归抽 EXIF 时间到 tab-separated 文件。
# Usage: 02_extract_exif.sh <source_dir> [work_dir=/tmp/tm]
# cwd MUST 是 tidymedia repo 根（bin/exiftool/exiftool.exe 相对路径）。
set -euo pipefail
SOURCE="${1:?missing source dir}"
WORK="${2:-/tmp/tm}"
mkdir -p "$WORK"

# -T：tab 分隔；空字段输出 `-`
# -q：抑制状态信息（保留 perl warning 到 stderr 不影响）
# CLAUDE.md 提示中文路径加 `-charset FileName=GBK`；本机 perl 缺 GBK 模块时
# 该 flag 反而报错，省掉也能正常输出（只是 stderr 多 locale 警告）。
# 字段顺序对齐 tidymedia P0..P4：DTO(P0 image)、QT:CreationDate(P0 video iPhone)、
# QT:CreateDate(P1 video)、CreateDate(P1 image)、FileModifyDate(P4)；
# Make/Model 用于 Step 5 pattern 分类。
# **MUST NOT** 加 `-fast2`：会跳过 QuickTime moov atom 致 QT 时间读不到，
# 老 QuickTime（pnot 起头 MOV）会被误判 tidymedia/exiftool 桶一致。
bin/exiftool/exiftool.exe -r -q -T \
    -p $'$Directory/$FileName\t$DateTimeOriginal\t$QuickTime:CreationDate\t$QuickTime:CreateDate\t$CreateDate\t$FileModifyDate\t$Make\t$Model' \
    "$SOURCE" \
    > "$WORK/exif.tsv" 2> "$WORK/exif.err"

# Windows perl 对含中文的入口路径按 ANSI(GBK) 输出文件名字节，下游
# compare_buckets.py 显式 encoding="utf-8" 会炸；统一规范化为 UTF-8。
uv run --quiet --no-project python - "$WORK/exif.tsv" <<'PYEOF'
import sys
p = sys.argv[1]
raw = open(p, "rb").read()
try:
    raw.decode("utf-8")
except UnicodeDecodeError:
    open(p, "w", encoding="utf-8", newline="\n").write(raw.decode("gbk"))
PYEOF

echo "exif_rows=$(wc -l < "$WORK/exif.tsv")"
echo "exif_err_lines=$(wc -l < "$WORK/exif.err")"
echo "work_dir=$WORK"
