#!/usr/bin/env bash
# Step 5：给单个文件写 AllDates + FileModifyDate，回读校验，并断言无 _original 残留。
# Usage: 05_write_exif.sh <file_path> <datetime>
# datetime 格式：YYYY:MM:DD HH:MM:SS（exiftool 标准）
# 调用前 MUST 由用户决策：时间精度、字段范围、是否保留备份。本脚本走「常用默认」：
#   - -overwrite_original：不留 _original 备份（避免 tidymedia 把它当媒体归档）
#   - -P：保留 FileAccessDate
#   - 同时写 AllDates 和 FileModifyDate，让 tidymedia 走 P0 路径
set -euo pipefail
FILE="${1:?missing file path}"
DT="${2:?missing datetime (YYYY:MM:DD HH:MM:SS)}"

bin/exiftool/exiftool.exe -P -overwrite_original \
    "-AllDates=$DT" "-FileModifyDate=$DT" "$FILE"

echo "---read back---"
bin/exiftool/exiftool.exe -time:all -s "$FILE" 2>&1 | grep -vE '^perl:|locale|Falling'

orig="${FILE}_original"
if [[ -e "$orig" ]]; then
    echo "WARNING: 残留备份 $orig（按 CLAUDE.md 应清理：magic-bytes 仍 JPEG，会被 tidymedia 归档）"
fi
