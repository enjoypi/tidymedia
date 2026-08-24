# Pattern 判定表与证据收集

Step 5 对候选集 U（MISMATCH ∪ DIFFER）逐文件归类。verify 已自动判定 5 个
（`verify.json` 的 `entries[].patterns`），其余需人工研判。

## verify 自动判定（diagnose.rs）

| Pattern | 触发条件 |
|---|---|
| `TidymediaContainerMiss` | `mismatch` 且 `exif_from ∈ {QTCreationDate, QTCreateDate}` | exiftool 读到容器时间但 tidymedia/nom-exif 漏读（如 pnot 起头老 QuickTime MOV），tidymedia 走 mtime 兜底桶错——tidymedia ≠ exiftool 的硬证据 |
| `CameraClockUnset` | `exif_exp_bucket` 以 `0000:` 开头 | 相机时钟未设，EXIF 时间不可信 |
| `FsTimeIsCopyStamp` | 决策含 `MtimeMuchEarlierThanP0` conflict | mtime 是拷盘/写卡时戳而非拍摄时间，P4 fallback 不可信 |
| `FilenameDateDiffers` | `filename_bucket` ≠ `actual_bucket` | 文件名/路径日期与决策桶冲突 |
| `ExactDuplicate` | `duplicate_verdict == "exact_dup"` | 目标库已有 SHA-512 相同副本 |

## 人工研判

| Pattern | 触发条件 | 含义 |
|---|---|---|
| `DefaultClockValue` | EXIF 三时间相同且形如 `YYYY:01:01 00:00:00`，且早于机型发布日 | 出厂默认值（CLAUDE.md「相机出厂默认时间陷阱」） |
| `ModelReleaseConflict` | EXIF 时间早于 Make/Model 已知发布日 | 时钟未设 + 残留出厂值，等同 DefaultClockValue |
| `PathDirectoryHint` | 路径父目录含可信日期片段 | 目录名是 ground truth；tidymedia 不解析路径，需手动补 EXIF |
| `FilenameStrong` | stem 含合法 `YYYY-MM-DD HH-MM-SS` | 相册命名风格，精度到秒 |
| `FilenameWeakDate` | stem 含合法 `YYYYMMDD` 但无 HHMMSS | 精度到日，HHMMSS 须默认 |
| `FilenameCoincidentalDigits` | stem 是 8 位数字但非合法日期 | 巧合 ID，MUST NOT 当日期用 |

## 证据收集（U 每个文件）

1. EXIF/容器全量：`bin/exiftool/exiftool.exe -s -G -time:all -Make -Model "<file>"`；
   视频如需 TrackCreateDate/MediaCreateDate/PreviewDate、图片如需 GPS 时间单独跑。
   **关注 exiftool ≠ tidymedia 硬证据**：`from=QTCreationDate`/`QTCreateDate` 报
   MISMATCH 即 tidymedia 漏读容器；`-MIMEType -FileType` 定位容器类型。
2. 路径目录暗示：扫每段父目录，匹配 `YYYY[.\-_]MM` / `YYYY年M月` / `YYYY年M-M月`
   （横跨多月 → 精度到年）/ 单独 `YYYY`（精度到年）。
3. 文件名暗示：`analyze_verify.ts` 已给 `filename_bucket`；8 位连号非合法日期标记
   `FilenameCoincidentalDigits`。

## 证据卡片模板

逐文件打印（MUST 全部列完再 AskUserQuestion）：

```markdown
### <relative path from source root>
- **EXIF 时间**: DTO=<v>, CreateDate=<v>, ModifyDate=<v>
- **容器时间**: QT:CreationDate=<v>, QT:CreateDate=<v>, Matroska:DateUTC=<v>
- **文件系统**: mtime=<v>, FileCreateDate=<v>
- **相机**: Make=<v>, Model=<v>
- **路径暗示**: <段路径> → <YYYY[:MM[:DD]]> | 无
- **文件名暗示**: <name=YYYY:MM 或 coincidental 或 无>
- **tidymedia 桶**: <YYYY:MM> (from=<DTO|QTCreationDate|QTCreateDate|CreateDate|FsMtime|NONE>)
- **exiftool 桶**: <YYYY:MM>（= analyze_verify 的 exp）
- **诊断**: `<Pattern1>` + `<Pattern2>` ...
- **推荐**: <写入值 YYYY:MM:DD HH:MM:SS> （理由：<最强可信线索>）| 跳过（理由：<无可信线索>）
```

推荐值优先级（高→低）：EXIF/容器时间合法 > FilenameStrong > FilenameWeakDate
（日 + 12:00:00）> PathDirectoryHint（日=1 + 12:00:00）> 跳过。MUST NOT 用
`FilenameCoincidentalDigits` 推。

`TidymediaContainerMiss` 场景：用 exiftool 读到的 QT:CreateDate 写回 AllDates +
FileModifyDate 让 tidymedia 下次走 P0；MUST 追问是否把容器解析缺口记 TODO.md。
