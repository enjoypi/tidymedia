---
description: tidymedia 归档「dry-run → EXIF 对账 → 文件名时间冲突排查 → EXIF 修补 → 真跑 move」全流程
argument-hint: <source_dir> <output_dir>
---

**原始参数字符串**：`$ARGUMENTS`

**MUST** 自行从字符串中解析出 `source_dir` 与 `output_dir`：Claude Code 内置的位置参数拆分器（`{dollar}1`/`{dollar}2`）对 Windows 反斜杠路径不可靠（`\U`/`\P` 等会被当 ANSI-C escape 吃掉、路径错位/为空），所以这里**只用 `$ARGUMENTS` 整串**。

解析规则：
- 按空格拆 2 token；若 token 两端是 `"`/`'`/`` ` ``，剥掉
- 路径内反斜杠 / 正斜杠原样保留（不要做任何 unescape）
- 任一解析为空 → 停下提示用法 `/tidy-verify <source_dir> <output_dir>`，不继续

下方脚本调用里的 `<SRC>` / `<OUT>` 是占位符，**MUST** 替换成解析出的实际路径再执行；双引号包裹避免 shell 二次解析。

cwd **MUST** 是 tidymedia repo 根（脚本里 `target/release/tidymedia.exe`、`bin/exiftool/exiftool.exe` 都是相对路径）。中间产物默认落 `/tmp/tm/`。

## Step 1：dry-run

```bash
.claude/scripts/tidy-verify/01_dry_run.sh "<SRC>" "<OUT>"
```

读 summary 行 `copied=N`、`copy_lines=N`，两者应相等。不等说明日志被截断或 grep 漏行，回头查。

## Step 2：抽 EXIF

```bash
.claude/scripts/tidy-verify/02_extract_exif.sh "<SRC>"
```

`exif_rows` 应等于 step 1 的 `summary.total`。

## Step 3：桶对账（EXIF 年月 vs target 桶）

```bash
.claude/scripts/tidy-verify/03_compare_buckets.sh
```

输出每条 MISMATCH：`exp=YYYY:MM  tgt=YYYY:MM  from=DTO|QTCreationDate|QTCreateDate|CreateDate|FsMtime|NONE  make=...  model=...  <source>`。

`from` 优先级对齐 tidymedia P0..P4：DTO > QTCreationDate > QTCreateDate > CreateDate > FsMtime。当 exiftool 能读到 `QT*Date` 但 tgt 桶 ≠ exp，意味着 **tidymedia/nom-exif 漏读了该容器时间**（最常见：pnot 起头老 QuickTime MOV、其他 nom-exif 不支持的容器），这是 tidymedia ≠ exiftool 的硬证据，Step 5 MUST 在证据卡片里标 `TidymediaContainerMiss` pattern。

**MUST NOT** 看到 MISMATCH 就直接 AskUserQuestion——先走完 Step 4 再进 Step 5 做证据卡片分析。

## Step 4：文件名时间冲突

source_root 由解析出的 `<SRC>` 末尾加分隔符得到（Windows 加 `\`、Unix 加 `/`）。

```bash
.claude/scripts/tidy-verify/04_filename_conflict.sh '<source_root_with_trailing_sep>'
```

DIFFER 行先收集到候选集，**不**逐条直接拍板；进 Step 5。

特殊场景：若 `with_name_time=0` 但 Step 3 仍报 MISMATCH（路径/文件名给不出日期），Step 5 仍 MUST 跑——只是证据卡片里「文件名暗示」会缺，Pattern 主要靠 EXIF/mtime/路径目录 / 相机机型仲裁。

## Step 5：证据收集 → 决策 → 写 EXIF

> 设计意图：用户调 tidy-verify 是为了**把可疑文件改对**，不是要被一堆 MISMATCH/DIFFER 数字砸脸。skill 在这一步要替用户把所有线索查清、按 pattern 归类、写成证据卡片，再让用户基于完整证据下决定。这样能把"相机时钟未设"和"文件名是巧合 ID"区分开，避免误改。

### 5.1 证据收集

待分析集合 U = Step 3 MISMATCH ∪ Step 4 DIFFER。对 U 每个文件：

1. **EXIF/容器全量**：脚本 03 输出已带 DTO/QT:CreationDate/QT:CreateDate/CreateDate/Make/Model；视频如需 TrackCreateDate/MediaCreateDate/PreviewDate、或图片如需 GPS 时间，单独跑：
   ```bash
   bin/exiftool/exiftool.exe -s -G -time:all -Make -Model "<file>"
   ```
   **关注 exiftool ≠ tidymedia 的硬证据**：`from=QTCreationDate` / `from=QTCreateDate` 报 MISMATCH 即 tidymedia 漏读了 nom-exif 不支持的容器；Make/Model 配合 `bin/exiftool/exiftool.exe -MIMEType -FileType <file>` 可定位容器类型（如 `pnot` 老 QuickTime → MIMEType=video/quicktime + nom-exif 漏读）。
2. **路径目录暗示**：扫 `<file>` 每一段父目录，匹配以下模式作为可信日期片段：
   - `YYYY[.\-_]MM` （如 `2012.10` / `2012-10` / `2012_10`）
   - `YYYY年M月` / `YYYY年MM月`
   - `YYYY年M-M月` / `YYYY年M月-M月`（横跨多月 → 精度只到年）
   - 单独 `YYYY` （精度到年）
3. **文件名暗示**：Step 4 已抽过 `name=YYYY:MM`；如果是 8 位连号但**不**是合法日期（`20070298_*` 等），标记为 `FilenameCoincidentalDigits` 而非有效日期。

### 5.2 Pattern 分类

按下表给每个文件叠加 0..N 个 pattern 标签（彼此可叠加，不互斥）：

| Pattern | 触发条件 | 含义 |
|---|---|---|
| `TidymediaContainerMiss` | Step 3 `from=QTCreationDate` 或 `from=QTCreateDate`，exp ≠ tgt | exiftool 读到容器时间但 tidymedia/nom-exif 漏读（如 pnot 起头 MOV），tidymedia 走 mtime 兜底，桶错；**最关键的 tidymedia ≠ exiftool 硬证据** |
| `CameraClockUnset` | EXIF DTO/CreateDate/ModifyDate 含 `0000:00:00` | 相机时钟未设，所有 EXIF 时间不可信 |
| `DefaultClockValue` | EXIF 三时间相同且形如 `YYYY:01:01 00:00:00`，且早于 Make/Model 机型发布日 | 出厂默认值，CLAUDE.md「相机出厂默认时间陷阱」 |
| `FsTimeIsCopyStamp` | mtime 是 `YYYY:01:01 00:00:00` 默认，或 mtime 早于 EXIF DTO > 30 天，或 FileCreateDate 远晚 mtime（>1 年） | mtime 是拷盘/写卡时戳而非拍摄时间，P4 fallback 不可信 |
| `PathDirectoryHint` | 路径父目录含可信日期片段 | 目录名是 ground truth；tidymedia 不解析路径，需手动补 EXIF |
| `FilenameStrong` | stem 含合法 `YYYY-MM-DD HH-MM-SS` | 文件名是相册命名风格，精度到秒 |
| `FilenameWeakDate` | stem 含合法 `YYYYMMDD` 但无 HHMMSS | 精度到日，HHMMSS 须默认 |
| `FilenameCoincidentalDigits` | stem 是 8 位数字但非合法日期 | 巧合 ID，**MUST NOT** 当日期用 |
| `ModelReleaseConflict` | EXIF 时间早于 Make/Model 已知发布日 | 时钟未设 + 残留出厂值，等同 DefaultClockValue |

### 5.3 证据卡片

对 U 每个文件，在对话里**逐文件**打印一段 markdown（**MUST 全部列完再问用户**，不要边打边问）：

```markdown
### <relative path from source root>
- **EXIF 时间**: DTO=<v>, CreateDate=<v>, ModifyDate=<v>
- **容器时间**: QT:CreationDate=<v>, QT:CreateDate=<v>, Matroska:DateUTC=<v>
- **文件系统**: mtime=<v>, FileCreateDate=<v>
- **相机**: Make=<v>, Model=<v>
- **路径暗示**: <段路径> → <YYYY[:MM[:DD]]> | 无
- **文件名暗示**: <name=YYYY:MM 或 coincidental 或 无>
- **tidymedia 桶**: <YYYY/MM> (from=<DTO|QTCreationDate|QTCreateDate|CreateDate|FsMtime|NONE>)
- **exiftool 桶**: <YYYY/MM>（从证据按 P0..P4 优先级推得 = 03 脚本 exp 字段）
- **诊断**: `<Pattern1>` + `<Pattern2>` ...
- **推荐**: <写入值 YYYY:MM:DD HH:MM:SS> （理由：<最强可信线索>）| 跳过（理由：<无可信线索>）
```

推荐值挑选优先级（高 → 低）：EXIF/容器时间合法（即 exiftool 桶可信） > FilenameStrong > FilenameWeakDate（日 + 12:00:00） > PathDirectoryHint（日=1 + 12:00:00） > 跳过。**MUST NOT** 用 `FilenameCoincidentalDigits` 推。

`TidymediaContainerMiss` 场景的推荐：用 exiftool 读到的 QT:CreateDate（HH:MM:SS 已精确）写回 AllDates + FileModifyDate，让 tidymedia 下次走 P0；同时**MUST**追问是否要把容器解析缺口记 TODO.md（参考已记录的 pnot QuickTime 自解析方案），下次发现新容器漏读类型时合并。

### 5.4 AskUserQuestion 决策

证据卡片全部呈现后，**MUST** 用 `AskUserQuestion` 至少问三件事（第一个选项 `全同意推荐 / 推荐`）：
- **批量策略**：全按推荐 / 逐个核 / 全跳过
- **HHMMSS 缺失时默认值**：`12:00:00`（避开午夜歧义，推荐）/ `00:00:00`
- **字段范围**：`AllDates + FileModifyDate`（推荐，走 P0）/ 仅 `AllDates`

如果证据矛盾（如 PathDirectoryHint=2012:10 但 EXIF=2007:01 且无 CameraClockUnset），**MUST** 单独问该文件信哪边——别合到批量里。

### 5.5 写 EXIF

按决策对每个待修文件调：

```bash
.claude/scripts/tidy-verify/05_write_exif.sh '<file_path>' 'YYYY:MM:DD HH:MM:SS'
```

脚本默认「不留备份 + 同写 AllDates 与 FileModifyDate」。需要保留 `_original` 或其他字段范围直接单跑 exiftool。

写完回 Step 1 重跑 dry-run，确认 MISMATCH/DIFFER 收敛到 0 或可接受残余。

## Step 6：真跑 move

**MUST** 用户显式 "move truly" / "真跑" 类同意后才执行。Move 物理删除源，不可逆。

```bash
.claude/scripts/tidy-verify/06_move_real.sh "<SRC>" "<OUT>"
```

脚本末尾打印 `remaining_files_in_source`（应 0）和 `empty_dirs_in_source`（按需手动清，tidymedia 不删空目录）。

## 陷阱速查

- `--log-level=debug`（带连字符），**不是** `--loglevel`；全局 flag 放最前
- `--dry-run` 是子命令级，**MUST** 放 `move` / `copy` 后面
- tidymedia stdout 不能 `| tail -N`：debug 日志走 stderr 一起被砍，summary 与 copy_file 行数对不上
- `awk -v VAR='D:\\...'` 会把 `\U`/`\P` 当 escape 吃掉反斜杠；Windows 路径**必须**通过环境变量传给 awk（脚本 4 已封装）
- gawk 变量名禁 `exp` / `log` / `length` 等内建函数名，syntax error 行号会指错
- EXIF naive 时间在 tidymedia 里按 `timezone_offset_hours`（默认 +8）转 epoch，归档桶再 `.to_offset(+8)` 取年月——首尾抵消，等于直接看 EXIF 字符串 `YYYY:MM`
- exiftool 写默认产生 `<file>_original` 备份，未清会被 tidymedia 当 JPEG 归档；脚本 5 默认 `-overwrite_original`
