# TODO

## 文件名启发式增补

- macOS 截图 `Screen Shot YYYY-MM-DD at HH.MM.SS` 格式（` at ` 分隔 + 时间用 `.` 点分隔）当前 `entities/media_time/filename.rs` 不解析，退到 mtime 归错桶。发现新样本时合并增补此模式（参考既有 `Screenshot_` 两种命名分支）。

## 容器/格式 EXIF 解析增补

- PNG `eXIf` chunk EXIF 当前 nom-exif 不解析：带 `DateTimeOriginal` 的 PNG（exiftool 可读）tidymedia 退到 mtime 归错桶（实测 `LKIT3149.png` DTO=2017:02 被归到 mtime 2021:05）。与 pnot QuickTime 自解析方案同思路，发现新样本累积后评估在 `entities/` 加 PNG eXIf chunk 自解析（PNG 1.5+ `eXIf` chunk = 裸 TIFF/EXIF IFD），或确认 nom-exif 上游支持后升级。
- Canon EOS 7D MakerNotes 偏移异常致 nom-exif EXIF 解析整体失败：exiftool 报 `Adjusted MakerNotes base by -126` 即此模式，DTO/CreateDate/Make/Model 全部读不到，tidymedia 退到 mtime 归错桶（实测 `D:\Users\Public\Pictures\2018\10\IMG_0545.JPG` DTO=2018:10:31 被归到 mtime 2018:11，本次 tidy-verify 通过 exiftool 重写 AllDates+FileModifyDate 强制 P4 兜底归对）。累积新样本后评估在 `entities/exif/image.rs::populate_image_dates` 失败路径下增加裸 TIFF/EXIF IFD0+ExifIFD 自解析作为 fallback，或确认 nom-exif 上游修复后升级。
