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

cwd **MUST** 是 tidymedia repo 根（`target/release/tidymedia` 相对路径）。

## 执行环境铁律

1. **禁 `rm`**：Windows Git Bash 下 `rm`/`cat`/`tail` 可能被映射到 bat 失效。
   工作目录用 UTC 时间戳子目录，每轮全新免清理；删文件用
   `bun -e 'import fs from "fs"; fs.rmSync(...)'`。`<WORK>` 取字面路径
   `/tmp/tm/<UTC 时间戳>`（如 `/tmp/tm/20261006T030000Z`），后续命令内联展开。

2. **前台 Bash + `timeout: 600000`**：实测 190 文件源 + 15.5 万文件目标库全流程
   约 8s。仅预计超 10 分钟（大源首轮、百 GB 真跑 move）才用 **Monitor**：
   输出重定向到 `<WORK>/` 文件，末尾 `; echo "exit=$?"` 作完成信号。

## Step 1：一次调用完成抽取 + 对账

```bash
bun .claude/skills/tidy-verify/scripts/run.ts "<SRC>" "<OUT>" "<WORK>"
```

`run.ts` 顺序执行：
- **exiftool 并行抽 tsv**：按目录文件数均衡分 `exiftool_concurrency` 组，每组
  argfile + `-charset filename=utf8` 一个进程，语义同 `-r`（跳 `.` 开头目录）。
  exiftool 缺失 → 打 `exiftool_missing=1`，省略 `--exif-tsv`（`mismatch` 恒
  false，verify 内部判定仍有效）。
- **`tidymedia verify`**：决策上浮 + 预测桶、tsv 交叉比对、文件名/路径日期桶、
  内容比对（`duplicate_verdict`）、pattern 诊断与 `fix_suggestion`。

stdout 直接打印完整对账汇总（summary 计数 / MISMATCH 明细 / DIFFER 明细 /
verdict 分布 / pattern 计数）与 `extract_ms` / `verify_ms`；产物 `exif.tsv`、
`verify.json`、`summary.txt`、`verify.err` 落 `<WORK>`。`mismatched>0` 或
`decision_failed>0` 时退出码非 0（预期内，继续流程）。桶格式统一 `YYYY:MM`
（口径见 `references/buckets.md`）。

**不做 move dry-run**：verify 的 `duplicate_verdict` 已覆盖重复判定；dry-run
要对 GB 级视频跑 SHA-512，是全流程最慢一步。

## Step 2：读汇总分流

- `MISMATCH_count=0` 且 `DIFFER_count=0` → 直进 Step 4。
- 否则进 Step 3。**MUST NOT** 见 MISMATCH 直接 AskUserQuestion。

## Step 3：证据卡片 → 决策 → 写 EXIF

> 用户调 tidy-verify 是为了把可疑文件改对。证据收集已脚本化，AI 只做研判与提问。

```bash
bun .claude/skills/tidy-verify/scripts/collect_evidence.ts "<WORK>/verify.json" "<SRC>" "<WORK>/evidence.md"
```

产出 `<WORK>/evidence.md`：候选集 U（MISMATCH ∪ DIFFER）逐文件证据卡片，除「推荐」
外全部字段已填（exiftool 全量时间 / 路径暗示 / 文件名暗示 / 诊断 patterns /
出厂默认时钟判定）。AI 逐卡片补「推荐」值——推荐值优先级与人工研判项
（`ModelReleaseConflict` 需机型发布日知识）见 `references/patterns.md`；拿不准的
用 `"/c/Windows/explorer.exe" "<file>"` 开图核实。

- **MUST** 全部卡片列完再 `AskUserQuestion`（至少三问，第一项推荐）：批量策略 /
  HHMMSS 缺失默认值（`12:00:00` 推荐）/ 字段范围（`AllDates + FileModifyDate`
  推荐）。
- 证据矛盾（如路径暗示 vs EXIF 冲突）MUST 单独问该文件信哪边。
- 写 EXIF（`references/exiftool.md` 陷阱，默认不留备份）：

```bash
bin/exiftool/exiftool.exe -P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=YYYY:MM:DD HH:MM:SS" "<file>"
```

写完回 Step 1 重跑 `run.ts`（复用同 `<WORK>`，tsv 重抽反映写回），确认
MISMATCH/DIFFER 收敛到 0 或可接受残余。

## Step 4：真跑 move

**MUST** 用户显式 "move truly" / "真跑" 类同意后才执行（物理删除源，不可逆）：

```bash
target/release/tidymedia --log-level=debug move --output "<OUT>" "<SRC>" > "<WORK>/run_real.log" 2>&1; echo "step4 exit=$?"
```

完成后核对源端清空（tidymedia 不删空目录，按需手动清）：

```bash
find "<SRC>" -type f | wc -l
find "<SRC>" -type d -empty | wc -l
```

## 陷阱速查

- `--log-level=debug`（带连字符）全局放最前；`--dry-run` 子命令级放 `move` 后。
- stdout 不能 `| tail`（debug 走 stderr 一起被砍）——重定向文件再 Read。
- 测试：`cd .claude/skills/tidy-verify && bun test`。
- 桶/时区口径见 `references/buckets.md`；exiftool 写回见 `references/exiftool.md`。
- 中文/空格路径拼 shell 一律双引号。
