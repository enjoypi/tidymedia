# TODO

## nom-exif fork：QuickTime MOV 兜底扩展

当前 `enjoypi/nom-exif` fork（branch `feat/legacy-quicktime-pnot`）只覆盖 **pnot 起头** 的老 QuickTime MOV。还有以下容器变体会让 tidymedia 漏读 `QT:CreateDate` 走 mtime 兜底（导致归档桶按写盘时间而非拍摄时间）：

- **mdat 起头**：首 box 直接是 `mdat`（无 ftyp/wide/pnot 头）
  - 案例：`D:\Users\Public\Pictures\2009\04\贝贝第一次去妈妈办公室.MOV`（QT:CreateDate=2009:03:31，mtime=2009:04:02）
  - 期望：mvhd.creation_time 解析逻辑与 pnot 路径共用 `mov.rs::extract_moov_body_from_buf`，仅需在 `parse_bmff_mime` 入口加 mdat 短路返 `video/quicktime`

合并到现有 pnot fork（PR 上游前一并提）。

## tidymedia 多数派仲裁误伤场景

当照片被第三方软件批量 re-save（EXIF DTO 保留原拍摄时间、EXIF ModifyDate + FileModifyDate 被改成 re-save 时间），且文件名按 re-save 时间命名时，会触发 P0OverruledByMajority 误把 DTO 推翻。

- 案例：`2009\02\2009-10-08 20-15-19.JPG`（DTO=2009:02:13 真拍摄，ModifyDate+mtime+filename=2009:10:08 是 re-save 时戳）
- 当前规避：tidy-verify 流程靠 PathDirectoryHint 人肉裁决，写 FileModifyDate 破仲裁
- 潜在改进：仲裁条件加 EXIF ModifyDate 维度——若 ModifyDate ≠ DTO 但 DTO 落在合理范围，降低多数派权重；或加 PathDirectoryHint 作第 4 维度仲裁
