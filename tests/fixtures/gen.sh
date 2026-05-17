#!/usr/bin/env bash
# 一次性 fixture 生成脚本。需要 ffmpeg + exiftool。生成产物 commit 到 tests/data/，
# 运行期不再依赖这两个工具。
#
# 用法：
#   bash tests/fixtures/gen.sh
#
# 设计原则：
#   - 每个产物对应 docs/media-time-detection.md 中某条具体规则
#   - 文件尽可能小（多数 < 2KB）
#   - 所有 EXIF/容器日期固定为可预测时间，便于 assert_eq
set -euo pipefail

DATA_DIR="$(cd "$(dirname "$0")/../data" && pwd)"
cd "$DATA_DIR"

#######################################
# P0 / 时区
#######################################

# JPEG: DateTimeOriginal + OffsetTimeOriginal (+08:00)
cp sample-with-exif.jpg sample-with-offset.jpg
exiftool -q -overwrite_original \
  -DateTimeOriginal="2024:05:01 14:30:00" \
  -OffsetTimeOriginal="+08:00" \
  -CreateDate="" -ModifyDate="" \
  sample-with-offset.jpg

#######################################
# P1 / Trap §5.4: 仅 ModifyDate，无 DateTimeOriginal/CreateDate
#######################################

cp sample-with-exif.jpg sample-modifydate-only.jpg
exiftool -q -overwrite_original \
  -DateTimeOriginal="" -CreateDate="" \
  -ModifyDate="2024:05:01 14:30:00" \
  sample-modifydate-only.jpg

#######################################
# Trap §5.1: MP4 1904-epoch（不传 creation_time → nom-exif 返回 1904-01-01）
#######################################

ffmpeg -y -loglevel error \
  -f lavfi -i color=s=8x8:d=0.1 -t 0.1 \
  -pix_fmt yuv420p \
  sample-mp4-1904-epoch.mp4

#######################################
# Trap §5.2: 未来时间（2099）
#######################################

cp sample-with-exif.jpg sample-future-2099.jpg
exiftool -q -overwrite_original \
  -DateTimeOriginal="2099:01:01 00:00:00" \
  -CreateDate="" -ModifyDate="" \
  sample-future-2099.jpg

#######################################
# Trap §5.3: 1995 之前
#######################################

cp sample-with-exif.jpg sample-pre-1995.jpg
exiftool -q -overwrite_original \
  -DateTimeOriginal="1980:01:01 00:00:00" \
  -CreateDate="" -ModifyDate="" \
  sample-pre-1995.jpg

#######################################
# P2 文件名（拷无 EXIF JPEG 再重命名，并追加唯一字节避免 hash 碰撞）
#######################################

cp sample-no-dates.jpg blank-noexif.jpg
trap 'rm -f "$DATA_DIR/blank-noexif.jpg"' EXIT

for name in \
  "IMG_20240501_143000.jpg" \
  "DSC_20240501_143000.jpg" \
  "Screenshot_2024-05-17-12-00-00.jpg" \
  "1715961600000.jpg"; do
  cp blank-noexif.jpg "$name"
  # 追加文件名 (含 NULL) 作为唯一后缀，让每个文件 hash 不同
  printf '\0%s' "$name" >> "$name"
done

#######################################
# P3 sidecar
#######################################

cat > sample-xmp.xmp <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
                     photoshop:DateCreated="2024-05-01T14:30:00+08:00"/>
  </rdf:RDF>
</x:xmpmeta>
EOF

cat > photo-takeout.json <<'EOF'
{"photoTakenTime":{"timestamp":"1714576200","formatted":"May 1, 2024, 2:30:00 PM UTC"}}
EOF

#######################################
# 清理临时
#######################################

rm -f blank-noexif.jpg
trap - EXIT

echo "Generated $(ls -1 "$DATA_DIR" | wc -l) files in $DATA_DIR"
