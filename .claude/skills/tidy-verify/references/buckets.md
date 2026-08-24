# 桶对账口径

verify 内部所有桶统一 `YYYY:MM` 字符串（`actual_bucket` / `exif_exp_bucket` /
`filename_bucket` 同格式），analyze_verify.ts 直接比较无转换。

## EXIF naive：首尾抵消

EXIF naive 时间（DTO/CreateDate）在 tidymedia 按 `timezone_offset_hours`（默认 +8）
转 epoch，归档桶再 `.to_offset(+8)` 取年月——首尾抵消，**直接看 EXIF 字符串前 7
字符 `YYYY:MM` 即桶**。`from` 优先级对齐 P0..P4：DTO > QTCreationDate >
QTCreateDate > CreateDate > FsMtime。

## QuickTime/视频：UTC 须 +tz

QuickTime 系字段（QT:CreationDate / QT:CreateDate）是 UTC 语义，对账须按配置时区
+N 取本地年月（verify 已内化 `bucket.rs::qt_bucket`，带 `±HH:MM`/`Z` 后缀先归一到
UTC 再 +tz）。「前 7 字符」规则仅对 EXIF naive 成立。

## dry-run 口径

`move --dry-run` 的 `copied=N` 只表示「目标库无 SHA-512 完全相同副本」，
**不等于新文件**——copied 里可能混 EXIF 已修版 / 旋转或重编码版 / 撞名版。
是否重复由 verify 的 `duplicate_verdict`（exact_dup / pixel_same / rotated_same /
name_only / absent）判别。

## 时区来源

`timezone_offset_hours` 从 tidymedia `config.yaml` 读（env
`TIDYMEDIA_TIMEZONE_OFFSET_HOURS` > config > 默认 8）。verify 内部处理，脚本侧无需
再取。

## 桶格式陷阱

- 归档目录是 `YYYY/MM`（斜杠），verify JSON 桶是 `YYYY:MM`（冒号）——两者只在
  「路径 vs 对账结果」对照时需要转换，analyze_verify 输出已是冒号格式。
- 预测桶由决策时间推得（`actual_bucket`），与 exiftool 期望桶
  （`exif_exp_bucket`）不一致即 `mismatch`。
