# Pattern 判定表与证据收集

Step 5 对候选集 U（MISMATCH ∪ DIFFER）逐文件归类。证据收集由
`scripts/collect_evidence.ts` 自动完成（exiftool 全量 dump + 路径/文件名暗示 +
出厂默认时钟判定），AI 只补「推荐」研判。verify 已自动判定 5 个
（`verify.json` 的 `entries[].patterns`），脚本再判 4 个，其余需人工研判。

## verify 自动判定（diagnose.rs）

| Pattern | 触发条件 |
|---|---|
| `TidymediaContainerMiss` | `mismatch` 且 `exif_from ∈ {QTCreationDate, QTCreateDate}` | exiftool 读到容器时间但 tidymedia/nom-exif 漏读（如 pnot 起头老 QuickTime MOV），tidymedia 走 mtime 兜底桶错——tidymedia ≠ exiftool 的硬证据 |
| `CameraClockUnset` | `exif_exp_bucket` 以 `0000:` 开头 | 相机时钟未设，EXIF 时间不可信 |
| `FsTimeIsCopyStamp` | 决策含 `MtimeMuchEarlierThanP0` conflict | mtime 是拷盘/写卡时戳而非拍摄时间，P4 fallback 不可信 |
| `FilenameDateDiffers` | `filename_bucket` ≠ `actual_bucket` | 文件名/路径日期与决策桶冲突 |
| `ExactDuplicate` | `duplicate_verdict == "exact_dup"` | 目标库已有 SHA-512 相同副本 |

## 脚本判定（collect_evidence.ts）

| Pattern | 触发条件 | 含义 |
|---|---|---|
| `DefaultClockValue` | EXIF 三时间相同且形如 `YYYY:01:01 00:00:00` | 出厂默认值（CLAUDE.md「相机出厂默认时间陷阱」） |
| `PathDirectoryHint` | 路径父目录含可信日期片段（`YYYY[.\-_]MM` / `YYYY年M月` / `YYYY年M-M月` 跨月→年精度 / 单独 `YYYY` 段→年精度） | 目录名是 ground truth；tidymedia 不解析路径，需手动补 EXIF |
| `FilenameStrong` | stem 含合法到秒时间（`YYYY-MM-DD HH-MM-SS` 分隔变体 / `YYYYMMDD_HHMMSS`） | 相册命名风格，精度到秒 |
| `FilenameWeakDate` | stem 含合法 `YYYYMMDD` 但无 HHMMSS | 精度到日，HHMMSS 须默认 |
| `FilenameCoincidentalDigits` | stem 含 8 位连号但非合法日期 | 巧合 ID，MUST NOT 当日期用 |

## 人工研判

| Pattern | 触发条件 | 含义 |
|---|---|---|
| `ModelReleaseConflict` | EXIF 时间早于 Make/Model 已知发布日 | 时钟未设 + 残留出厂值，等同 DefaultClockValue |

## 证据卡片（脚本生成，`推荐` 待研判）

`collect_evidence.ts` 对 U 逐文件输出：

```markdown
### <relative path from source root>
- **EXIF 时间**: DTO=<v>, CreateDate=<v>, ModifyDate=<v>
- **容器时间**: QT:CreationDate=<v>, QT:CreateDate=<v>, Matroska:DateUTC=<v>
- **文件系统**: mtime=<v>, FileCreateDate=<v>
- **相机**: Make=<v>, Model=<v>
- **路径暗示**: <段路径> → <YYYY[:MM[:DD]]> | 无
- **文件名暗示**: <name=YYYY:MM 或 coincidental 或 无>
- **tidymedia 桶**: <YYYY:MM> (from=<DTO|QTCreationDate|QTCreateDate|CreateDate|FsMtime|NONE>)
- **exiftool 桶**: <YYYY:MM>（= verify 汇总的 exp）
- **诊断**: `<Pattern1>` + `<Pattern2>` ...
- **推荐**: (待研判)
```

推荐值优先级（高→低）：EXIF/容器时间合法 > FilenameStrong > FilenameWeakDate
（日 + 12:00:00）> PathDirectoryHint（日=1 + 12:00:00）> 跳过。MUST NOT 用
`FilenameCoincidentalDigits` 推。

`TidymediaContainerMiss` 场景：用 exiftool 读到的 QT:CreateDate 写回 AllDates +
FileModifyDate 让 tidymedia 下次走 P0；MUST 追问是否把容器解析缺口记 TODO.md。
