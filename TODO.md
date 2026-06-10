# TODO

## MPEG-TS/M2TS（AVCHD .MTS）容器不支持

nom-exif 完全不支持 MPEG-TS 容器；拍摄时间内嵌在 H.264 流（SEI/GOP，exiftool 读作 `H264:DateTimeOriginal`），tidymedia 走 mtime 兜底，mtime 是拷贝戳（如 FAT 默认 1980:01:01）时归错桶。

- 案例：`D:\Users\Public\Pictures\2011\10\2011-10贝贝安安动物园之行\201110_0007[1-4].MTS` 等 6 个 Canon AVCHD（exiftool DTO=2011:10，mtime=1980:01:01 → 归 1980/01）
- 注意：M2TS 不可写 EXIF，数据侧修复只能写 FileModifyDate（tidy-verify 已按此处理）
- 与上面 QuickTime 缺口不同源：需独立的 TS packet → PES → H.264 SEI 解析，工作量大；先观察出现频率再决定自解析还是换库
