# Pattern 判定与证据研判

Step 3 对 MISMATCH、DIFFER 和 decision_failed 条目逐文件研判。`collect_evidence.ts` 自动收集前两类的时间、路径、文件名和相机信息；仅有 decision_failed 的条目按 SKILL 手工补齐。AI 只补充推荐与需要用户确认的冲突。

## verify 自动判定

| Pattern | 触发条件 | 含义 |
|---|---|---|
| `TidymediaContainerMiss` | mismatch 且 exif_from 为 QuickTime 来源 | exiftool 读到容器时间而 tidymedia 未采用，需核查容器解析 |
| `CameraClockUnset` | exif_exp_bucket 以 `0000:` 开头 | 相机时钟未设置，时间不可信 |
| `FsTimeIsCopyStamp` | 决策含 MtimeMuchEarlierThanP0 冲突 | mtime 可能是复制时间，不宜作为拍摄时间 |
| `FilenameDateDiffers` | filename_bucket 与 actual_bucket 不同 | 文件名或路径日期与当前决策冲突 |
| `ExactDuplicate` | duplicate_verdict 为 exact_dup | 目标库已有内容完全相同的副本 |

## collect_evidence.ts 判定

| Pattern | 触发条件 | 含义 |
|---|---|---|
| `DefaultClockValue` | EXIF 三时间相同且形如 `YYYY:01:01 00:00:00` | 可能是出厂默认值，需结合机型和其他证据 |
| `PathDirectoryHint` | 父目录含可信年月或日期片段 | 重要旁证，不是自动覆盖其他证据的真相来源 |
| `FilenameStrong` | stem 含合法且精确到秒的时间 | 可作为强文件名证据 |
| `FilenameWeakDate` | stem 只有合法日期 | 可作为日级证据，时分秒需用户确认默认值 |
| `FilenameCoincidentalDigits` | 连续数字形似日期但不合法 | 不得当作日期 |

“合法日期”必须是真实日历日期。当前脚本只做初步数字校验，AI 必须额外核对月份天数和闰年；例如 `20230231`、非闰年 2 月 29 日和小月 31 日都不得作为证据。

## 人工研判

| Pattern | 触发条件 | 含义 |
|---|---|---|
| `ModelReleaseConflict` | EXIF 时间早于已知机型发布日 | 可能未设置时钟或残留出厂值 |

## 证据卡片

脚本为 MISMATCH/DIFFER 条目生成卡片，decision_failed 条目由 AI 按 SKILL 手工补齐。卡片包含相对路径、EXIF/容器/文件系统时间、相机、路径和文件名暗示、双方桶、诊断及待填推荐。合并后必须覆盖全部候选文件，再集中提问。

推荐顺序：经证据确认可信的拍摄或容器时间 > FilenameStrong > FilenameWeakDate > PathDirectoryHint > 跳过。出厂默认值、机型冲突或其他反证优先否决看似合法的时间；证据相互矛盾时必须询问用户，不机械套用顺序。

只有日期时默认建议 `12:00:00`；只有年月或年时，默认日期和时刻也必须由用户确认。`FilenameCoincidentalDigits` 永不能推出时间。

`TidymediaContainerMiss` 可向用户报告疑似解析缺口，并按已核实的容器时间提出写回建议；未核实前不得修改归档规则或把旧调查结论当作事实。
