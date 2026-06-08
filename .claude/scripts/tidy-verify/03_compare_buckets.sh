#!/usr/bin/env bash
# Step 3：EXIF 年月 vs tidymedia 推断的 target 桶 YYYY/MM。
# Usage: 03_compare_buckets.sh [work_dir=/tmp/tm]
# 前置：work_dir 内已有 exif.tsv（step 2）与 copy_lines.log（step 1）。
set -euo pipefail
WORK="${1:-/tmp/tm}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

awk -f "$SCRIPT_DIR/compare_buckets.awk" \
    "$WORK/exif.tsv" "$WORK/copy_lines.log" \
    > "$WORK/bucket_result.txt"

echo "---summary---"
tail -2 "$WORK/bucket_result.txt"
mismatch=$(grep -c '^MISMATCH' "$WORK/bucket_result.txt" || true)
echo "MISMATCH_count=$mismatch"
if [[ "$mismatch" -gt 0 ]]; then
    echo "---by from---"
    grep '^MISMATCH' "$WORK/bucket_result.txt" | awk -F'\t' '{print $4}' | sort | uniq -c
    echo "---details (first 20)---"
    grep '^MISMATCH' "$WORK/bucket_result.txt" | head -20
fi
