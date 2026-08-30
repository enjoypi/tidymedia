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
   `bun -e 'import fs from "fs"; fs.rmSync(...)'`：

   ```bash
   export WORK="/tmp/tm/$(date -u +%Y%m%dT%H%M%SZ)" && mkdir -p "$WORK"
   ```

   Bash/Monitor 每次调用都是新 shell，env 不持久：记下生成的字面路径，后续
   命令内联展开（`<WORK>` 占位符同义）。

2. **长步骤 MUST Monitor 后台**：dry-run / verify / exiftool 抽取都是全量扫描，
   必超 2 分钟；Bash 前台与 `run_in_background` 都会被 2 分钟杀掉，只有
   **Monitor 工具**能跑长任务。模式：command 重定向输出到 `$WORK/` 文件，
   末尾 `; echo "exit=$?"` 作完成信号，`timeout_ms` 按规模给足
   （百 GB 量级给 1800000），收到完成通知后 Read 输出文件。

## 对账核心：tidymedia verify

对账确定性逻辑已内化 `tidymedia verify <SRC> -o <OUT> [--exif-tsv]`：决策上浮 +
预测桶、注入 tsv 交叉比对、文件名/路径日期桶、内容比对（`duplicate_verdict`）、
pattern 诊断与 `fix_suggestion`。**stdout 直接打印完整对账汇总**（summary 计数 /
MISMATCH 明细 / DIFFER 明细 / verdict 分布 / pattern 计数），无需独立分析脚本。
`mismatched>0` 或 `decision_failed>0` 时 `$?` 非 0（预期内，继续流程）。桶格式
统一 `YYYY:MM`（口径见 `references/buckets.md`）。

skill 保留 exiftool 交叉验证的抽 tsv（第二实现独立性是交叉比对的价值所在）与
写回（verify 只诊断不写盘）。

## Step 1+2（两个 Monitor 并行）：dry-run 规模 + 抽 exiftool tsv

两者无依赖，同时起：

```bash
target/release/tidymedia --log-level=debug move --dry-run --output "<OUT>" "<SRC>" > "$WORK/run.log" 2>&1; echo "step1 exit=$?"
```

```bash
bun .claude/skills/tidy-verify/scripts/extract_exif.ts "<SRC>" "$WORK" > "$WORK/extract.log" 2>&1; echo "step2 exit=$?"
```

完成后 Read：`run.log` 末尾 summary 行 `copied=N`（只表示目标库无 SHA-512 相同
副本，不等同新文件，重复判别看 Step 3 的 `duplicate_verdict`）；`extract.log` 的
`exif_rows` 应等于 summary.total，exiftool 缺失退出 2 → Step 3 省略
`--exif-tsv`（`mismatch` 恒 false，verify 内部判定仍有效）。

## Step 3：verify 对账（Monitor 后台）

```bash
target/release/tidymedia --log-level=debug verify "<SRC>" -o "<OUT>" --exif-tsv "$WORK/exif.tsv" --report "$WORK/verify.json" > "$WORK/summary.txt" 2> "$WORK/verify.err"; echo "step3 exit=$?"
```

## Step 4：读汇总分流

Read `$WORK/summary.txt`：

- `MISMATCH_count=0` 且 `DIFFER_count=0` → 直进 Step 6。
- 否则进 Step 5。**MUST NOT** 见 MISMATCH 直接 AskUserQuestion。

## Step 5：证据卡片 → 决策 → 写 EXIF

> 用户调 tidy-verify 是为了把可疑文件改对。证据收集已脚本化，AI 只做研判与提问。

```bash
bun .claude/skills/tidy-verify/scripts/collect_evidence.ts "$WORK/verify.json" "<SRC>" "$WORK/evidence.md" > "$WORK/evidence.log" 2>&1; echo "step5 exit=$?"
```

产出 `$WORK/evidence.md`：候选集 U（MISMATCH ∪ DIFFER）逐文件证据卡片，除「推荐」
外全部字段已填（exiftool 全量时间 / 路径暗示 / 文件名暗示 / 诊断 patterns /
出厂默认时钟判定）。AI 逐卡片补「推荐」值——推荐值优先级与人工研判项
（`ModelReleaseConflict` 需机型发布日知识）见 `references/patterns.md`；拿不准的
用 `"/c/Windows/explorer.exe" "<file>"` 开图核实。

- **MUST** 全部卡片列完再 `AskUserQuestion`（至少三问，第一项推荐）：批量策略 /
  HHMMSS 缺失默认值（`12:00:00` 推荐）/ 字段范围（`AllDates + FileModifyDate`
  推荐）。
- 证据矛盾（如路径暗示 vs EXIF 冲突）MUST 单独问该文件信哪边。
- 写 EXIF（`references/exiftool.md` 陷阱，默认不留备份；单文件秒级可前台）：

```bash
bin/exiftool/exiftool.exe -P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=YYYY:MM:DD HH:MM:SS" "<file>"
```

写完回 Step 3 重跑 verify（复用同 `$WORK`），确认 MISMATCH/DIFFER 收敛到 0 或
可接受残余。

## Step 6：真跑 move（Monitor 后台）

**MUST** 用户显式 "move truly" / "真跑" 类同意后才执行（物理删除源，不可逆）：

```bash
target/release/tidymedia --log-level=debug move --output "<OUT>" "<SRC>" > "$WORK/run_real.log" 2>&1; echo "step6 exit=$?"
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
