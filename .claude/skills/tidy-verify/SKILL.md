---
name: tidy-verify
description: 归档前核对拍摄时间、预测归档位置和重复关系，收集异常证据，经用户确认后修复时间并正式移动。
argument-hint: <source_dir> <output_dir>
---

# tidy-verify：归档前对账

**原始参数字符串**：`$ARGUMENTS`

自行解析 `source_dir` 与 `output_dir`，不要使用内置位置参数拆分器。仅使用双引号分组：成对首尾双引号去除，引号内空格属于同一参数，其他字符包括 Windows 反斜杠均原样保留。必须恰好得到两个非空参数；无法唯一解析时，提示 `/tidy-verify <source_dir> <output_dir>` 并停止。下文 `<SRC>`、`<OUT>` 为占位符。

cwd 必须是 tidymedia 仓库根目录。

## 执行环境

1. 禁止使用 `rm`。每次 tidy-verify 任务使用新的 `<WORK>=/tmp/tm/<UTC 时间戳>`，避免清理旧目录；同一任务内写回后的复核对同一目录覆盖更新。确需删除时使用 Bun 文件 API。
2. 前台 Bash 使用 `timeout: 600000`。仅预计超过 10 分钟才改用 Monitor；输出重定向到 `<WORK>/`，命令末尾附加 `; echo "exit=$?"` 作为完成信号。
3. 中文或含空格路径在 shell 中统一加双引号。日志重定向到文件后读取，不用管道截断输出。
4. 若操作系统时区与 `timezone_offset_hours` 不同，FsMtime 的 exiftool 对账可能偏离实际归档桶；暂停依赖该结果的写回和移动，先统一时区或修正对账口径。

## Step 1：抽取并对账

```bash
bun .claude/skills/tidy-verify/scripts/run.ts "<SRC>" "<OUT>" "<WORK>"
```

`run.ts` 依次执行：

- 并行调用 exiftool，按目录文件数和可用内存分组，使用 argfile 与 UTF-8 文件名输出；TSV 先分组写入再合并。残留进程按 pid 文件清理。
- 调用 `tidymedia verify`，生成预测归档桶、时间决策、冲突、内容重复裁定、诊断和修复建议。

stdout 输出汇总和耗时；产物按执行情况写入 `<WORK>`，包括 `verify.json`、`summary.txt`、`verify.err`，成功抽取时另有 `exif.tsv`。`mismatched > 0` 或 `decision_failed > 0` 会产生非零退出码，这是需要继续研判的对账结果，不是脚本崩溃。

exiftool 缺失时输出 `exiftool_missing=1`，省略 `--exif-tsv` 并继续 verify；此时没有第二实现交叉比对，内部判定仍有效。执行过抽取时必须确认 `exif_exit=0`；非零表示交叉比对不完整，立即停止后续写回和移动，不得把残缺 TSV 当作完整证据，即使脚本仍继续调用 verify。

桶格式统一为 `YYYY:MM`，详见 `references/buckets.md`。不要另跑 move dry-run；verify 已提供重复裁定，而 dry-run 会对大文件重复判重。

## Step 2：按汇总分流

- `MISMATCH_count=0`、`DIFFER_count=0`、`decision_failed=0` 且没有执行错误：进入 Step 4。
- 任一时间异常计数非零：进入 Step 3。`decision_failed` 单独非零时也必须研判，不得因另外两个计数为 0 而跳过。
- 抽取、verify 或报告存在执行错误：先停止并排除错误，不进入正式移动。不得仅因出现 MISMATCH 就直接向用户提问，必须先收集完整证据。

## Step 3：研判、确认、写回

```bash
bun .claude/skills/tidy-verify/scripts/collect_evidence.ts "<WORK>/verify.json" "<SRC>" "<WORK>/evidence.md"
```

证据收集缺少 exiftool 时退出 2，不能假装已获得第二实现证据。脚本只为 MISMATCH 与 DIFFER 的并集生成证据卡片；仅有 `decision_failed` 的文件必须另行逐文件运行 exiftool，按同一卡片格式补齐时间、文件名和路径证据。AI 根据 `references/patterns.md` 补齐推荐值，无法确定时打开原文件人工核实。

任一 exiftool 调用报错、证据卡片意外全空或关键字段均为 `<无>` 时，必须停止并逐文件手工核查，不得把工具失败当作“已收集完整证据”。

- 必须列完全部卡片后再用 AskUserQuestion 集中确认。至少覆盖批量策略、缺少时分秒时的默认值、写字段范围；选项使用数字编号，第 1 项为推荐项。
- 路径、文件名和时间元数据冲突时，必须单独询问用户信任哪项证据。
- 默认建议时间为 `12:00:00`，字段范围为 AllDates + FileModifyDate，是否保留 `_original` 由用户选择。
- 用户确认写回后才可执行 exiftool。Windows 使用仓内 `bin/exiftool/exiftool.exe`，macOS 使用已安装的 `exiftool`；下列 `<EXIFTOOL>` 按平台替换：

```bash
<EXIFTOOL> -P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=YYYY:MM:DD HH:MM:SS<OFFSET>" "<file>"
```

`<OFFSET>` 是配置时区偏移，例如 UTC+8 写 `+08:00`。`-overwrite_original` 表示不留备份，只有用户选择该方式时才使用。视频必须同时核对并修正 QuickTime CreationDate 与 CreateDate，不能只写较低优先级字段；详见 `references/exiftool.md`。

写回后返回 Step 1，复用同一 `<WORK>` 重新抽取并对账，确认异常收敛到 0 或仅剩用户接受的项。

## Step 4：正式移动

只有用户明确表达“真跑”“正式移动”等同意后才可执行。移动会物理删除源文件，不可撤销。执行前检查最新 verify：未接受的 MISMATCH、DIFFER、decision_failed 和执行错误必须已清零，任何残余都有用户的明确决定。

```bash
target/release/tidymedia --log-level=debug move --output "<OUT>" --report "<WORK>/move.json" "<SRC>" > "<WORK>/run_real.log" 2>&1; echo "step4 exit=$?"
```

Windows 可执行文件使用 `target/release/tidymedia.exe`。完成后检查日志、`move.json` 的 `failed`、`errors` 和实际移动计数，以及源端剩余文件；tidymedia 不删除空目录，空目录由用户另行处理。

```bash
find "<SRC>" -type f | wc -l
find "<SRC>" -type d -empty | wc -l
```

测试：`cd .claude/skills/tidy-verify && bun test`。
