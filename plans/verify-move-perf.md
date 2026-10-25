# verify / move 性能分析与优化计划

## Context

目标场景：`tidymedia verify D:\old -o D:\user\Pictures` 和 `tidymedia move D:\old -o D:\user\Pictures`（同卷 local→local，Windows 11 + NTFS + Defender）。

数据规模：
- Pictures：约 15.5 万文件，697 GB。
- D:\old：≥ 50.9 万文件，多数是 js、svg、json、无扩展名文件，像 node_modules 和备份；媒体约 jpg 2.2 万、png 2.8 万。

调研（3 个 Explore + 3 个 Plan，其中一个从批判角度）的结论：
- 没有实测基线。docs 里唯一的数据是 verify 一次运行：System 23.7s、User 4.5s。
- perf-collect 跑 move 时强制 `--dry-run`，而 dry-run 会对每个源文件额外算一次全量 SHA-512，测到的热点和真跑不一样。
- 发现 move 有会丢数据的缺陷（R 已核实代码）：
  - R：并行同名竞争。`copy_loop.rs:79` 按桶并行；`naming.rs:87` 的 `contains_target` 检查和 `ops_phantom.rs:87` 的 `add` 不是原子操作，同名但 hash 不同的两个源会 rename 到同一目标，后者覆盖前者，而前者的源已经删掉。
  - A：walker 默认 `hidden(true)`（`local_backend_impl.rs:47`），output 中的隐藏文件不进索引；Windows 上 `fs::rename` 带 `MOVEFILE_REPLACE_EXISTING`，会直接覆盖。
  - B：`contains_target`（`file_index.rs:126`）区分大小写，而 NTFS 不区分。
  - D：`under_prefix` 区分大小写，`move d:\x -o D:\x\out` 会把已归档文件当成重复项删掉。
  - C：去重在 `passes_type_filter` 之前（`ops_phantom.rs:33-48`），与 output 重复的非媒体源文件会被删。

用户已确认：
1. 先修安全，再做优化。
2. 收窄源范围（零代码），同时在代码里加扩展名黑名单。
3. 先测量再改。
4. verify 要能跑大源（约 5 万个媒体文件）。

## 阶段 0：测量基线（零代码，不碰原数据）

1. **硬链接沙箱**：执行 `cp -al /d/old /d/perf/src` 和 `cp -al /d/user/Pictures /d/perf/out`。真跑 move 时只对沙箱操作；每轮重跑前重建 src；结束后 `rm -rf /d/perf`。
2. **源范围统计**：用只读的 PowerShell 按目录统计媒体文件数，排除 `node_modules`、`.git`，把清单交给用户挑选 source 子目录。
3. **分阶段测量**，每组跑 3 次，记录首次和中位数：
   - 只测 walk：`[IO.Directory]::EnumerateFiles` 计时。
   - move 真跑：`tidymedia move D:\perf\src -o D:\perf\out --log-level debug --report r.json 2> err.log`，用 perf-collect 里的 PowerShell 采样脚本取 wall/user/sys。从 `scanned source files` 和 `summary` 两条日志的时间戳拆出各阶段耗时；把 `-o` 换成空目录，量出目标库索引的成本；再对比 `--log-level error` 在控制台下的 wall。
   - verify：用 `bun scripts/perf-collect.ts --sub verify`，源分别取 1、190、1900 个媒体文件，算截距（目标库建索引）和斜率（每个源文件的耗时）。
   - Defender 归因：`New-MpPerformanceRecording` 和 `Get-MpPerformanceReport -TopExtensions/-TopFiles`；`wpr -start CPU -start FileIO`。
4. **判定阈值**（决定阶段 2 里各项做不做）：
   - verify 斜率 > 5 ms/源 → 做 P1。
   - verify 截距 > 5 s，且 CreateFile 次数 ≈ 目标库文件数 × 2 → 做 P6。
   - 每个源文件 CreateFile ≥ 3 次，且平均 ≥ 50 µs → 做 P5。
   - samply 里 section/map 相关占比 < 10% → 放弃把 mmap 换成 read。
   - walk 占 wall > 30% → 做 P7。
   - 去重 SHA-512 读取量达几十 GB 且磁盘吃满 → 记录为已知上限，不做无谓优化。
5. **测量产物**放 scratchpad，结论写进 `docs/performance.md`，追加到已有文档。

## 阶段 1：数据安全修复（先写失败测试，按主题拆 commit）

- **S1 不覆盖写入**：新增 `src/adapters/backend/local_noreplace.rs`。
  - Windows：`MoveFileExW(old,new,0)`。windows-sys 改为直接依赖，按主版本锁定。
  - Unix：`hard_link` + `remove_file`；remove 失败时用 `partial_move_error` 包装。
  - `open_write` 改为 `create_new(true)`。
  - 跨卷回退：`create_new` + `io::copy` + `set_modified`。
  - Backend trait 写明「冲突返回 `AlreadyExists`」；`FakeBackend` 同步模拟。SMB 远端暂不做，记为后续。
- **S2 目标名原子占位**：`Index::try_reserve_target`，用 DashSet，Windows 下 key 用 casefold，替换 `naming.rs:87` 的检查；`do_copy` 遇到 `AlreadyExists` 时重新调 `generate_unique_name`，受 `unique_name_max_attempts` 限制。dry-run 走同一路径。
- **S3 防重叠自删**：Windows 下 `under_prefix`（`common_phantom.rs`）按 ASCII 忽略大小写；删源前若 `canonicalize(dup)==canonicalize(src)`，拒绝删除并报错。
- **S4 先过滤再去重**：把 `passes_type_filter` 移到 `exists` 之前（`ops_phantom.rs`），commit message 注明行为变更。
- **守卫测试**：
  - output 中有隐藏的同名文件；
  - Windows 上 `IMG.JPG` 和 `img.jpg`；
  - 两个同名、hash 不同的源并行（Fake 的 rename 加闸门，稳定复现竞争）；
  - rename 到已存在目标返回 `AlreadyExists`；
  - 跨卷回退保留 mtime；
  - `move d:\x -o D:\x\out` 后 out 里的文件仍在；
  - 非媒体重复文件不删源。
- **后续 P2（本轮不做）**：walker 改为 `hidden(false)` 加 `skipped_hidden` 计数提示。

## 阶段 2：性能优化（按阶段 0 的结论取舍，先写失败测试）

| # | 内容 | 关键位置 | 复用 |
|---|---|---|---|
| P1 | verify：`lookup_tsv` 改用 `HashMap`，重复 key 时第一条胜出；`find_candidates` 改用新建的 `CandidateIndex`（`exact`：小写全名，`variant`：去掉 stem 末尾 `_\d+`），旧的线性实现留在测试里作对照 | `verify/mod.rs:203`、新建 `verify/content_diff_candidates.rs` | `split_stem_ext`/`basename`/`stem_digit_variant` |
| P2 | verify 主循环并行：先收集 key，再 `par_iter` 调 `evaluate_one`，结果按 `source_path` 排序后串行汇总计数；候选文件字节只读一次，源图惰性解码一次（拆成 `prepare_src`/`matches`）；图像解码用 CPU 池，避免 IO 池 64 线程把内存撑大 | `verify/mod.rs:80`、新建 `verify/evaluate.rs`、`content_diff.rs:42-57,164-218` | `install_io`/threadpool |
| P3 | 非媒体和空文件的 `warn!` 降为 `debug!`；`CopyDelta` 加 `skipped_non_media` 计数，汇总时打一条 warn；Report 字段按同步检查点处理 | `ops.rs:33-52`、`copy_loop.rs:186`、`file_index_build.rs:247` | `log_operation_summary` |
| P4 | 扩展名黑名单：新配置 `copy.skip_extensions`（js/json/svg/css/ts/html/xml/map），只对 source 生效，且仅在 `!include_non_media && !doc_only` 时生效；`dat` 和无扩展名文件不进黑名单；新增 `VisitStats::skipped_by_ext` | `file_index_build.rs:256` 的 walk 循环、`usecases/config.rs`、`config.yaml`、`frameworks/config.rs::sanitize_*` | 按新增配置字段的同步检查点 |
| P5 | 每个源文件只 open 一次：`Entry` 带上 walk 时拿到的 metadata；新增 `Info::open_with`，免掉一次 stat；新增 `Backend::read_head`（Local 用 `File::read`），一次读取同时完成 fast_hash 和 256B 的 sniff；`Exif::open_filtered_with_mime` 拿到 mime 后对非媒体直接返回；size 对不上时退回重新 stat | `entities/backend/mod.rs:34`、`info.rs:90`、`streams.rs:218`、`mime.rs:32`、`types.rs:106`、`file_index_enrich.rs:42` | `to_metadata`、`read_fill` |
| P6 | output 索引惰性化：walk 时把全部路径（含 0 字节和不可读文件）放进 `known`，按 size 分到 `pending`；`exists` 发现 size 命中时，才对该桶里的文件执行 `Info::open`（clone 出 Arc 后释放 guard 再做 IO）；`contains_target` 改查 `known`。依赖 S1/S2 | 新建 `entities/file_index_lazy.rs`、`file_index_build.rs:31,182`、`copy_loop.rs:38,43`、`verify/content_diff.rs:69,125` | — |
| P7 | sidecar 只对媒体文件探测；walk 改用 `build_parallel`（看阶段 0 的结论再定） | `file_index_enrich.rs:63`、`copy/index.rs:71`、`local_backend_impl.rs:47` | — |
| P8 | tidy-verify skill：给 exiftool 加 `-ext` 媒体白名单，跳过 node_modules | `.claude/skills/tidy-verify/scripts/lib/extract.ts`、`config.yaml` | — |

不做：`config()` 的 Mutex 改 OnceLock，经评估收益不到 1 ms，而且会破坏 `reset_config_loader`。

实施顺序：S1→S2→S3→S4→P1→P3→P4→P2→P5→P6→P7/P8。每项完成后重跑阶段 0 的同一组测量，对比收益并写进 `docs/performance.md`。

约束：文件 < 512 行，函数 < 64 行；不写注释；覆盖率四项都要 100%（走 rust-cov-100 门禁）；commit 用 `type: 中文描述`，按主题拆分。

## 验证

1. 每项：`just test <filter>` 先红后绿；完成后 `just test` 全量、`just lint`、rust-cov-100 门禁。
2. 安全：在硬链接沙箱里真跑 `move D:\perf\src -o D:\perf\out`，然后检查：
   - out 中原有文件数和抽样 hash 不变；
   - 源文件数 = 已归档文件数 + 去重删除数 + 跳过数；
   - 没有任何文件被覆盖：比对 out 原有文件的 mtime 与 size 快照。
3. 性能：重跑阶段 0 的 move 和 verify 测量，产出前后对比表（wall、user、sys、CreateFile 次数、Defender 扫描时长）。
4. verify 等价性：用同一组源分别跑优化前和优化后的 verify，`report.json` 的 entries 排序后逐字段相同。
5. 计划批准后，把本文件移到 `D:\prj\tidymedia\plans\verify-move-perf.md`。
