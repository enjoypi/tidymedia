# TODO

## 自解析老 QuickTime atom 树（pnot 起头 MOV）

**现状**：`d0c76e3` 已让 `pnot` 起头的老 QuickTime MOV（如 NIKON COOLPIX P5000）通过 MIME 嗅探兜底识别为 `video/quicktime`，避免被 `is_media=false` 误判 ignore。但 nom-exif 同样不解析这种容器，`populate_video_dates` 拿不到 `TrackInfoTag::CreateDate`，归档桶 fallback 到 P4 mtime——与 exiftool 读到的 `[QuickTime] CreateDate` 可能差几天到几个月（2007 实测样本：mtime 2007:08:05 vs QuickTime CreateDate 2007:07:24）。

**长期方案**：仿 `entities/riff.rs` 对 AVI 的做法，在 `entities/` 加 `quicktime.rs` 自解析 QuickTime atom 树，按 atom size + 4 字节 tag 扫描，递归进入 `moov > mvhd` 提取 32-bit `creation_time`（QuickTime epoch = 1904-01-01 UTC，要减 2_082_844_800 转 Unix epoch）；在 `Exif::from_reader` 的 `video/quicktime` 分流里，当 nom-exif 返回零候选时调用兜底。

**对账**：补 fixture（3 张 DSCN MOV 或等价的 pnot 起头小样本）到 `tests/data/`，端到端断言 `Info::create_time` ≈ exiftool 读到的 QuickTime CreateDate；覆盖率口径维持 region/line/branch 100%。

**优先级**：低。`pnot` MOV 在主流相机里早已淘汰，2007 之前老相机才常见；当前 fallback P4 mtime 在多数场景"差不到一个月"够用，遇到精度敏感样本可临时用 `bin/exiftool/exiftool.exe '-FileModifyDate<CreateDate' -overwrite_original <file>` 把 QuickTime CreateDate 同步到 mtime 再 tidymedia。
