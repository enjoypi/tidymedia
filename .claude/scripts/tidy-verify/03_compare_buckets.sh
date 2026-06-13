#!/usr/bin/env bash
# Step 3：EXIF 年月 vs tidymedia 推断的 target 桶 YYYY/MM。
# Usage: 03_compare_buckets.sh [work_dir=/tmp/tm]
# 前置：work_dir 内已有 exif.tsv（step 2）与 copy_lines.log（step 1）。
set -euo pipefail
WORK="${1:-/tmp/tm}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# QuickTime 容器时间是 UTC，对账需按配置时区 +N 取本地年月（详见 compare_buckets.py）。
# 时区优先级对齐 tidymedia：env TIDYMEDIA_TIMEZONE_OFFSET_HOURS > config.yaml 默认值 > 8。
# config.yaml 用 tidymedia 自定义 env 展开语法 `${VAR:-N}`（非标准 yaml），正则提取默认 N。
TZ_HOURS=$(uv run python - <<'PY'
import os, re

v = os.environ.get("TIDYMEDIA_TIMEZONE_OFFSET_HOURS")
if not v:
    try:
        txt = open("config.yaml", encoding="utf-8").read()
        m = re.search(
            r"timezone_offset_hours:\s*(?:\$\{[^:}]*:-(-?\d+)\}|(-?\d+))", txt
        )
        v = (m.group(1) or m.group(2)) if m else "8"
    except OSError:
        v = "8"
print(v or "8")
PY
)

uv run "$SCRIPT_DIR/compare_buckets.py" \
    "$WORK/exif.tsv" "$WORK/copy_lines.log" "$TZ_HOURS" \
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
