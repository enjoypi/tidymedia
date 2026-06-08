#!/usr/bin/env bash
# Step 1：跑 tidymedia move --dry-run 并抽 copy_file 行。
# Usage: 01_dry_run.sh <source_dir> <output_dir> [work_dir=/tmp/tm]
# cwd MUST 是 tidymedia repo 根（target/release/tidymedia.exe 相对路径）。
set -euo pipefail
SOURCE="${1:?missing source dir}"
OUTPUT="${2:?missing output dir}"
WORK="${3:-/tmp/tm}"
mkdir -p "$WORK"

# 重定向到文件而非 | tail：dry-run 逐文件 debug 日志体量大，pipe 截断会让
# copy_file 行数 < summary.copied，对账时一头雾水。
target/release/tidymedia.exe --log-level=debug move --dry-run \
    --output "$OUTPUT" "$SOURCE" \
    > "$WORK/run.log" 2>&1
grep -E 'operation="copy_file"' "$WORK/run.log" > "$WORK/copy_lines.log"

echo "summary:"
tail -1 "$WORK/run.log"
echo "copy_lines=$(wc -l < "$WORK/copy_lines.log")"
echo "work_dir=$WORK"
