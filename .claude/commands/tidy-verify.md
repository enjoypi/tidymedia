---
description: tidymedia 归档「dry-run → EXIF 对账 → 文件名时间冲突排查 → EXIF 修补 → 真跑 move」全流程
argument-hint: <source_dir> <output_dir>
---

参数：source_dir = `$1`，output_dir = `$2`。

如任一为空，停下提示用法：`/tidy-verify <source_dir> <output_dir>`，不要继续。

cwd **MUST** 是 tidymedia repo 根（脚本里 `target/release/tidymedia.exe`、`bin/exiftool/exiftool.exe` 都是相对路径）。中间产物默认落 `/tmp/tm/`。

## Step 1：dry-run

```bash
.claude/scripts/tidy-verify/01_dry_run.sh "$1" "$2"
```

读 summary 行 `copied=N`、`copy_lines=N`，两者应相等。不等说明日志被截断或 grep 漏行，回头查。

## Step 2：抽 EXIF

```bash
.claude/scripts/tidy-verify/02_extract_exif.sh "$1"
```

`exif_rows` 应等于 step 1 的 `summary.total`。

## Step 3：桶对账（EXIF 年月 vs target 桶）

```bash
.claude/scripts/tidy-verify/03_compare_buckets.sh
```

输出每条 MISMATCH：`exp=YYYY:MM  tgt=YYYY:MM  from=DTO|CreateDate|FsMtime|NONE`。`from=FsMtime` 兜底的多半值得进 step 4 看是否文件名能补 EXIF。

## Step 4：文件名时间冲突

source_root 由 `$1` 末尾加分隔符得到（Windows 加 `\`、Unix 加 `/`）。

```bash
.claude/scripts/tidy-verify/04_filename_conflict.sh '<source_root_with_trailing_sep>'
```

逐条 DIFFER 肉眼判断：文件名是常见相册命名风格（含合法 YYYYMMDD/HHMMSS）→ 修 EXIF；8 位数字是巧合 ID → 用户决策。

## Step 5：补 EXIF

**MUST** 用 `AskUserQuestion` 让用户拍板：
- 时间精度（按文件名抽完整 HHMMSS / 仅日期 + 12:00:00 / 仅日期 + 00:00:00）
- 是否保留 `_original` 备份（CLAUDE.md：未清理会被 tidymedia 当 JPEG 归档）
- 字段范围（默认 AllDates + FileModifyDate，让 tidymedia 走 P0）

按用户决策对每个目标文件调：

```bash
.claude/scripts/tidy-verify/05_write_exif.sh '<file_path>' 'YYYY:MM:DD HH:MM:SS'
```

脚本默认「不留备份 + 同写 AllDates 与 FileModifyDate」。需要其他变体直接单跑 exiftool 或改脚本。

写完回 Step 1 重跑 dry-run，确认目标桶按预期改成 P0/P1 推断结果，Step 3/4 不再报相关 MISMATCH/DIFFER。

## Step 6：真跑 move

**MUST** 用户显式 "move truly" / "真跑" 类同意后才执行。Move 物理删除源，不可逆。

```bash
.claude/scripts/tidy-verify/06_move_real.sh "$1" "$2"
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
