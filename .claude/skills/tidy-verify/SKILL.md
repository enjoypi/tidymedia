---
name: tidy-verify
description:
argument-hint: <source_dir> <output_dir>
---

# tidy-verify：归档前对账

**原始参数字符串**：`$ARGUMENTS`

**MUST** 自行解析 `source_dir` 与 `output_dir`：Claude Code 内置位置参数拆分器对
Windows 反斜杠路径不可靠，只用 `$ARGUMENTS` 整串。解析规则：按空格拆 2 token，
token 两端引号剥掉，反斜杠/正斜杠原样保留；任一为空 → 提示
`/tidy-verify <source_dir> <output_dir>` 停下。下方 `<SRC>`/`<OUT>` 是占位符。

cwd **MUST** 是 tidymedia repo 根（`target/release/tidymedia` 相对路径）。工作目录
`<WORK>` 默认 `config.yaml` 的 `work_dir`（`/tmp/tm`），每轮开头清空。

## 对账核心：tidymedia verify

Step 1/3/4/4.5 的确定性逻辑已内化为 `tidymedia verify <SRC> -o <OUT> [--exif-tsv]`
：决策上浮 + 预测桶（`actual_bucket`）、注入 tsv 交叉比对（`mismatch`/
`exif_exp_bucket`/`exif_from`/`make`/`model`）、文件名/路径日期桶
（`filename_bucket`）、内容比对（`duplicate_verdict`）、pattern 诊断与
`fix_suggestion`。`mismatched>0` 或 `decision_failed>0` 时 `$?` 非 0。桶格式统一
`YYYY:MM`（口径见 `references/buckets.md`）。

skill 保留 exiftool 交叉验证的抽 tsv 与写回（verify 只诊断不写盘）。

## Step 1：dry-run 归档规模

```bash
rm -rf "$WORK"/* && mkdir -p "$WORK"
target/release/tidymedia --log-level=debug move --dry-run --output "<OUT>" "<SRC>" > "$WORK/run.log" 2>&1
```

读 summary 行 `copied=N`。copied 只表示「目标库无 SHA-512 相同副本」，不等同新文件
（重复判别看 Step 4 的 `duplicate_verdict`）。

## Step 2：抽 exiftool tsv

```bash
bun .claude/skills/tidy-verify/scripts/extract_exif.ts "<SRC>" "$WORK"
```

`exif_rows` 应等于 Step 1 的 `summary.total`。exiftool 缺失（macOS 未装）→ 脚本退
出 2，跳过交叉比对继续（verify 内部判定仍有效）。

## Step 3：verify 对账

```bash
target/release/tidymedia --log-level=debug verify "<SRC>" -o "<OUT>" --exif-tsv "$WORK/exif.tsv" --report "$WORK/verify.json"
```

无 exiftool 时省略 `--exif-tsv`（`mismatch` 恒 false，仅内部判定）。

## Step 4：汇总分析

```bash
bun .claude/skills/tidy-verify/scripts/analyze_verify.ts "$WORK/verify.json"
```

输出 MISMATCH（`exp`/`tgt`/`from`/`make`/`model`）、DIFFER（`name`/`tgt`）、
`duplicate_verdict` 分布、`patterns` 计数。MISMATCH 与 DIFFER 收集进候选集 U，
进 Step 5 逐文件证据卡片。**MUST NOT** 见 MISMATCH 直接 AskUserQuestion。

## Step 5：证据收集 → 决策 → 写 EXIF

> 用户调 tidy-verify 是为了把可疑文件改对。Step 5 替用户查清所有线索、按 pattern
> 归类、写证据卡片，再基于完整证据下决定。

- 证据收集、pattern 判定表、证据卡片模板、推荐值优先级 →
  `references/patterns.md`。
- **MUST** 全部证据卡片列完再 `AskUserQuestion`（至少三问，第一项推荐）：
  批量策略 / HHMMSS 缺失默认值（`12:00:00` 推荐）/ 字段范围（`AllDates +
  FileModifyDate` 推荐）。
- 证据矛盾（如路径暗示 vs EXIF 冲突）MUST 单独问该文件信哪边。
- 写 EXIF（`references/exiftool.md` 陷阱，默认不留备份）：

```bash
bin/exiftool/exiftool.exe -P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=YYYY:MM:DD HH:MM:SS" "<file>"
```

写完回 Step 3 重跑 verify，确认 MISMATCH/DIFFER 收敛到 0 或可接受残余。

## Step 6：真跑 move

**MUST** 用户显式 "move truly" / "真跑" 类同意后才执行（物理删除源，不可逆）：

```bash
target/release/tidymedia --log-level=debug move --output "<OUT>" "<SRC>" > "$WORK/run_real.log" 2>&1
```

末尾核对 `remaining_files_in_source`（应 0）与 `empty_dirs_in_source`（按需手动清，
tidymedia 不删空目录）：

```bash
find "<SRC>" -type f | wc -l
find "<SRC>" -type d -empty | wc -l
```

## 陷阱速查

- `--log-level=debug`（带连字符）；全局 flag 放最前；`--dry-run` 子命令级放 `move`
  后。stdout 不能 `| tail`（debug 走 stderr 一起被砍）。
- 测试：`cd .claude/skills/tidy-verify && bun test`。
- 桶/时区口径见 `references/buckets.md`；exiftool 写回见 `references/exiftool.md`。
- 中文/空格路径拼 shell 一律双引号；Git Bash 无 `explorer`/`cmd` 时用
  `"/c/Windows/explorer.exe" "<file>"` 打开核图。
