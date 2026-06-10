# TODO

## MPEG-TS/M2TS（AVCHD .MTS）容器不支持

nom-exif 完全不支持 MPEG-TS 容器；拍摄时间内嵌在 H.264 流（SEI/GOP，exiftool 读作 `H264:DateTimeOriginal`），tidymedia 走 mtime 兜底，mtime 是拷贝戳（如 FAT 默认 1980:01:01）时归错桶。

- 案例：`D:\Users\Public\Pictures\2011\10\2011-10贝贝安安动物园之行\201110_0007[1-4].MTS` 等 6 个 Canon AVCHD（exiftool DTO=2011:10，mtime=1980:01:01 → 归 1980/01）
- 注意：M2TS 不可写 EXIF，数据侧修复只能写 FileModifyDate（tidy-verify 已按此处理）
- 与上面 QuickTime 缺口不同源：需独立的 TS packet → PES → H.264 SEI 解析，工作量大；先观察出现频率再决定自解析还是换库

## tidymedia 多数派仲裁误伤场景

当照片被第三方软件批量 re-save（EXIF DTO 保留原拍摄时间、EXIF ModifyDate + FileModifyDate 被改成 re-save 时间），且文件名按 re-save 时间命名时，会触发 P0OverruledByMajority 误把 DTO 推翻。

- 案例：`2009\02\2009-10-08 20-15-19.JPG`（DTO=2009:02:13 真拍摄，ModifyDate+mtime+filename=2009:10:08 是 re-save 时戳）
- 当前规避：tidy-verify 流程靠 PathDirectoryHint 人肉裁决，写 FileModifyDate 破仲裁
- 潜在改进：仲裁条件加 EXIF ModifyDate 维度——若 ModifyDate ≠ DTO 但 DTO 落在合理范围，降低多数派权重；或加 PathDirectoryHint 作第 4 维度仲裁
