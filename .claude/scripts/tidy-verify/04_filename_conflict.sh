#!/usr/bin/env bash
# Step 4：源路径里嵌入的时间字符串 vs target 桶。
# Usage: 04_filename_conflict.sh <source_root_with_trailing_sep> [work_dir=/tmp/tm]
# 前置：work_dir 内有 copy_lines.log（step 1）。
# 注意：source_root MUST 以路径分隔符结尾（例：'D:\Users\Public\Pictures\2006\'），
# 否则 awk 的前缀剥离失败会让根目录里的年份段污染 parse_path。
set -euo pipefail
SOURCE_ROOT="${1:?missing source root (must end with separator)}"
WORK="${2:-/tmp/tm}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# SOURCE_ROOT 必须通过 env 传给 awk，不能 `awk -v`：awk -v 会把 `\U`/`\P` 当
# escape 序列吃掉 Windows 路径反斜杠。
SOURCE_ROOT="$SOURCE_ROOT" awk -f "$SCRIPT_DIR/filename_conflict.awk" \
    "$WORK/copy_lines.log" > "$WORK/name_result.txt"

echo "---summary---"
tail -2 "$WORK/name_result.txt"
differ=$(grep -c '^DIFFER' "$WORK/name_result.txt" || true)
echo "DIFFER_count=$differ"
if [[ "$differ" -gt 0 ]]; then
    echo "---details---"
    grep '^DIFFER' "$WORK/name_result.txt"
fi
