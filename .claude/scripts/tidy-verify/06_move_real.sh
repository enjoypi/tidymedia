#!/usr/bin/env bash
# Step 6：真跑 move（无 --dry-run）。
# Usage: 06_move_real.sh <source_dir> <output_dir> [work_dir=/tmp/tm]
# 调用前 MUST 由用户给出显式同意（"move truly" / "真跑" 类指令）。
# move 物理删除源，不可逆。
set -euo pipefail
SOURCE="${1:?missing source dir}"
OUTPUT="${2:?missing output dir}"
WORK="${3:-/tmp/tm}"
mkdir -p "$WORK"

target/release/tidymedia.exe --log-level=debug move \
    --output "$OUTPUT" "$SOURCE" \
    > "$WORK/run_real.log" 2>&1
rc=$?

echo "exit=$rc"
echo "---summary---"
tail -1 "$WORK/run_real.log"

err=$(grep -cE 'result="(error|failed)"' "$WORK/run_real.log" || true)
echo "error_log_lines=$err"

remaining=$(find "$SOURCE" -type f 2>/dev/null | wc -l)
empty=$(find "$SOURCE" -type d -empty 2>/dev/null | wc -l)
echo "remaining_files_in_source=$remaining"
echo "empty_dirs_in_source=$empty  (tidymedia 不删空目录，按需手动清)"

exit "$rc"
