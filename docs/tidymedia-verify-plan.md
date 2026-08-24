# tidymedia-verify 实现计划（2026-08-25 已批准 → **已实现**）

> 把当前项目 skill「tidy-verify」业务尽量实现到 tidymedia（Rust CLI）。范围 = `.claude/commands/tidy-verify.md` + `.claude/scripts/tidy-verify/`（9 个脚本）；全局 skill 均判定无交集，排除。
>
> **实现状态（2026-08-25）**：阶段 1-5 全部落地，`tidymedia verify` 子命令可用（1947 测试全过 + clippy/fmt 干净）。落地内容：`usecases/verify/`（mod/bucket/exif_tsv/filename_hint/content_diff/diagnose/report）+ `Info::media_time_decision()` 决策上浮 + 装配 9 处 + `--exif-tsv` 注入 + 内容比对（EXACT_DUP/PIXEL_SAME/ROTATED）+ 5 pattern 诊断 + 修补建议 + skill 联动。`regex` 依赖新增。

## 目标形态

新增 `tidymedia verify <source> <output> [--report <f>] [--exif-tsv <f>] [--phash-max <N>] [--include-non-media]`，把 tidy-verify 约 90% 确定性业务内化为子命令。

## 用户拍板

1. EXIF 读源 = 内置读（nom-exif 全栈）+ 可选 `--exif-tsv` 注入（保留第二实现交叉验证，不破 R1）
2. EXIF 写回 = verify 只出修补清单（建议 exiftool 命令），写回动作留 skill
3. v1 范围 = 含内容像素比对全套（EXACT_DUP + PIXEL_SAME 熵流 hash + 旋转校正 pHash）

## 关键设计决策

- **交叉验证保持**：`exp` 列默认来自注入 exiftool tsv（skill 02 产出）；未注入退化「预测桶 vs 实际桶」内部一致 + conflict + pattern
- **PathDirectoryHint 不进决策**：仅诊断 HINT 输出，不进入 `MediaTimeDecision`（避免改变归档桶行为）
- **R1 不破**：生产路径零 `std::process::Command`；exiftool 仅在 skill 侧（02 抽 tsv / 05 写回）
- **内容比对语义**：对 output 目录已归档文件比对

## 结构（触达「新增子命令」检查点全部 9 处）

| 层 | 文件 | 改动 |
|---|---|---|
| CLI | `src/adapters/cli.rs` | `Commands::Verify{...}` |
| dispatch | `src/adapters/dispatch.rs` | `CommandResult::Verify` + tidy partial arm + tidy_with arm + `dispatch_verify` |
| report | `src/usecases/report.rs` | `Report::Verify(&VerifyReport)` + `FEATURE_VERIFY`（单点 use） |
| sink | `src/adapters/report_sink.rs` | `Report::Verify` 写盘 arm → JSON |
| usecases | `usecases/verify/`（新目录） | `mod.rs` + 子模块：`bucket.rs`/`exif_tsv.rs`/`filename_hint.rs`/`content_diff.rs`/`diagnose.rs`/`report.rs` |
| entities | `entities/file_info/info.rs` | 抽 `pub fn media_time_decision()`（现 create_time 丢弃 decision，`info.rs:214-274`） |
| lib | `src/lib.rs` | re-export |

## 子模块迁移来源

| 子模块 | 来源 |
|---|---|
| `bucket.rs` | `compare_buckets.py:30-99`（P0..P4 首个非空 / QT UTC→+tz / 0000 回退 / 双分隔符 target） |
| `exif_tsv.rs` | `02_extract_exif.sh` -p 8 列契约 |
| `filename_hint.rs` | `filename_conflict.py:37-64` + PathDirectoryHint（三正则 + 目录段） |
| `content_diff.rs` | `45_check_copied.py` 全量（EXACT_DUP / SOS 熵流 / IDAT / mdat 三态 / 旋转 pHash 四向 min hamming ≤10 + 尺寸 + 像素 diff mean<5） |
| `diagnose.rs` | `tidy-verify.md:99-113`（9 pattern） |
| `report.rs` | VerifyReport / VerifyEntry |

## 阶段（同属 v1，每阶段 nextest + 覆盖率 4×100% 收敛）

1. **骨架 + 决策上浮**：`media_time_decision()` 抽出 + verify 空壳 + 9 检查点装配 + e2e + JSON 断言
2. **桶对账**：`bucket.rs` + `exif_tsv.rs` 注入交叉比对
3. **文件名冲突 + 目录段提示**：`filename_hint.rs` + Filename pattern，踩坑用例转单测
4. **内容像素比对**：`content_diff.rs`（最大阶段）
5. **诊断引擎 + 修补清单**：9 pattern + fix_suggestion + JSON/TSV 终版 + 更新 tidy-verify.md 引用

## 保持外部

- exiftool 抽 tsv（02）→ verify `--exif-tsv` 注入源
- EXIF 写回（05）→ verify 出修补清单，写回留 skill
- 真跑 move（06）→ 已有 `move` 子命令

## 同步检查点 MUST

新增子命令 9 处 / Report 字段同步（`elapsed_ms` 单点 / `push_error_capped` / `scanned` 同 CopyReport 口径）/ CLI flag e2e 双侧 / `Location::join_path` / `canonical_prefix` / tracing feature 单点 / 覆盖套路过关。

## Fixture

小 JPEG(SOS 熵流变体)/PNG(IDAT)/微型 MP4(mdat 32/64/EOF 三态)/旋转 90° 图对/`西宁 2008-6-19 13-08-21.jpg`/exif.tsv 8 列样本/踩坑用例集（P1120296、YY-MM-DD）。

## Verification

每阶段 `cargo nextest run --release` + Linux `--branch` 4×100%；e2e JSON 断言 + 与 compare_buckets.py/filename_conflict.py 输出对拍；`--exif-tsv` 注入实测 tsv 判定一致。

## 风险

旋转 pHash 大图内存（`max_image_bytes` guard 同加）；`VerifyReport.entries` 无上限 → report JSON 聚合 + stdout TSV 逐文件双通道；exif-tsv 8 列契约漂移 → `exif_tsv.rs` + skill 02 注释互指。