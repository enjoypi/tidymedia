# 桶对账口径

verify 内部桶统一为 `YYYY:MM` 字符串，`actual_bucket`、`exif_exp_bucket`、`filename_bucket` 使用相同格式，stdout 汇总直接比较。

## EXIF naive：时区转换相互抵消

无时区的 EXIF DTO/CreateDate 先按 `timezone_offset_hours` 转为时间点，归档时再按同一配置时区取年月，因此可直接用 EXIF 字符串前 7 个字符 `YYYY:MM` 对账。

`exif_from` 表示 exiftool 期望桶采用的时间来源，选择顺序与 verify 的 TSV 契约一致；不要把文件名或 sidecar 候选混入该字段。

## QuickTime 与视频：先按 UTC 换算

QuickTime CreationDate/CreateDate 是 UTC 语义。带 `±HH:MM` 或 `Z` 后缀的值先归一到 UTC，再按配置时区取本地年月。EXIF naive 的“前 7 个字符”规则不适用于 UTC 容器时间。

## dry-run 与内容裁定

`move --dry-run` 的 `copied=N` 只表示目标库没有内容完全相同的副本，不等于都是全新内容；其中可能有时间写回、旋转或重编码版本。

是否重复以 verify 的 `duplicate_verdict` 为准，包括 exact_dup、pixel_same、rotated_same、name_only、absent 等结果。不要只用文件名或预测桶判断重复。

## 时区与路径格式

- `timezone_offset_hours` 由 verify 从配置读取，脚本无需重复解析。
- 归档目录使用 `YYYY/MM`，verify JSON 桶使用 `YYYY:MM`；只有对照路径和对账结果时才转换格式。
- `actual_bucket` 来自 tidymedia 决策，`exif_exp_bucket` 来自注入 TSV；两者不一致即产生 mismatch。
- `exif_from=FsMtime` 当前直接使用 exiftool 字符串的年月，只有操作系统时区与配置时区一致时才可与实际归档桶比较；不一致时先停止该对账，不得通过手工改月份掩盖问题。
