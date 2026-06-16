# TODO

## 文件名启发式增补

- macOS 截图 `Screen Shot YYYY-MM-DD at HH.MM.SS` 格式（` at ` 分隔 + 时间用 `.` 点分隔）当前 `entities/media_time/filename.rs` 不解析，退到 mtime 归错桶。发现新样本时合并增补此模式（参考既有 `Screenshot_` 两种命名分支）。

## 容器/格式 EXIF 解析增补

- PNG `eXIf` chunk EXIF 当前 nom-exif 不解析：带 `DateTimeOriginal` 的 PNG（exiftool 可读）tidymedia 退到 mtime 归错桶（实测 `LKIT3149.png` DTO=2017:02 被归到 mtime 2021:05）。与 pnot QuickTime 自解析方案同思路，发现新样本累积后评估在 `entities/` 加 PNG eXIf chunk 自解析（PNG 1.5+ `eXIf` chunk = 裸 TIFF/EXIF IFD），或确认 nom-exif 上游支持后升级。
