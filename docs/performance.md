# tidymedia 性能测试指南

面向 **AI 分析真实数据性能** 的测试手段。所有产物机器可读，可直接扔给 LLM 提问；不产 SVG 火焰图或需人打开的二进制 profile。

## 快速开始

```bash
# 一条命令产完整报告（用真实照片库测 find 子命令）
bun scripts/perf-collect.ts \
  --sub find \
  --data /path/to/your/photos \
  --output-dir /tmp/perf-run
```

产物写入 `--output-dir`：

| 文件 | 内容 | 消费方 |
|---|---|---|
| `perf-report.md` | 单一 markdown 汇总（L1 + L4） | LLM 直接读入 |
| `report.json` | tidymedia `--report` 落地的原始 JSON（含 `duration_ms`） | `jq` / LLM |
| `time-v.txt` | `/usr/bin/time -v` 抓的 stderr 原文 | 备份参考 |

`--sub` 支持：`copy` / `move` / `find` / `cull` / `move-text-shot` / `verify`。`copy` / `move` / `move-text-shot` 默认加 `--dry-run` 不写目标；`verify` 必带 `--output-target <目标库>`。

## 采集层级

### L1：Report duration_ms（use case 内耗时）

所有 4 个 Report（`CopyReport` / `FindReport` / `CullReport` / `MoveTextShotReport`）+ FFI 侧 `MobileFindReport` 均含 `duration_ms: u64` 字段：从 usecase 入口 `Instant::now()` 到构造 Report 的 wall-clock 毫秒。单点计算：`usecases::report::elapsed_ms(start)`。

**注意**：`duration_ms` 不含 Rust 启动 + ONNX 模型加载等固定开销；那些落在 L4 `elapsed_wall` 里。

### L4：/usr/bin/time -v（系统资源）

`perf-collect.ts` 用 `/usr/bin/time -v` 包裹 tidymedia 进程，抓以下字段：

| 字段 | 含义 |
|---|---|
| `Elapsed (wall clock)` | 整进程总耗时（含启动/加载） |
| `Maximum resident set size` | 峰值 RSS，OOM 风险指示 |
| `User time` / `System time` | CPU 分布：System 占比高 → 系统调用密集 |
| `Percent of CPU` | CPU 利用率，> 100% 说明多核并行 |
| `File system inputs/outputs` | block 数（512 B/block） |
| `Major page faults` | 触发磁盘换页数，非零表示 RAM 不足 |
| `Minor page faults` | 内存分配次数，可推 allocator 压力 |
| `Voluntary/Involuntary context switches` | 主动 IO 等待 vs 被动抢占 |

## AI 分析模板

把 `perf-report.md` + `report.json` + `time-v.txt` 一起贴给 Claude/GPT，用下述 prompt 起头：

```
以下是 tidymedia <sub> 命令在 <N> 张真实照片上的性能采集报告。请回答：

1. 瓶颈类型：IO 受限 / CPU 受限 / 内存受限？依据哪几个指标？
2. 吞吐 <MiB/s> 相对文件类型（多为 JPEG/HEIC/MP4）是否合理？
3. `duration_ms` 与 `elapsed_wall` 差值属正常启动开销还是过大？
4. `Major page faults > 0` 时给出减小内存驻留的具体代码建议
5. `System CPU / User CPU` > 30% 时定位可能的高频系统调用
6. 是否需要调整 `STREAM_CHUNK` / `MIME_SNIFF_BYTES` / rayon 线程池大小？
```

## 常见分析路径

**吞吐低但 CPU 空闲** → IO 受限。查看 `fs_inputs` 是否远大于文件总大小（重复读同文件？未命中 page cache？）。

**吞吐低且 CPU 满** → CPU 受限。热点通常在 SHA-512 或 ONNX 推理。可选：
- SHA-512 → 评估 blake3 迁移（现约定 SHA-512 单点，改动大）
- ONNX → 检查 `into_optimized` vs `into_typed` 分流是否正确

**RSS 峰值 >> 峰值私有内存**（Windows）→ 本地 mmap 读的文件页计入工作集，可回收，非堆分配；以私有内存判断内存压力。

**System CPU >> User CPU** → 系统调用密集。verify 实测 System 23.7 s / User 4.5 s：目标库每文件 stat ×2 + open + 4 KiB 读。

**RSS 峰值 > 文件总大小** → 缓冲/驻留过多。查 `MAX_REMOTE_WRITE_BUFFER` / `STREAM_CHUNK` 配置。

**Major page faults > 10** → 已开始换页。减小并发 worker 数或用更小 `STREAM_CHUNK`。

**Wall clock >> duration_ms** → 固定开销大（ONNX 模型加载）。仅 `cull` / `move-text-shot` 走 ONNX；其他子命令启动开销应 < 500 ms。

**逐文件 `warn!` 刷 stderr** → 同一批非媒体文件 dry-run，默认级别 2 min 08 s vs `--log-level error` 33 s（77 万行 stderr，64 线程争用无缓冲 stderr）。逐文件事件 MUST `debug!`，用户可见提示走一条汇总 `warn!`。

## Windows 实测（D:\old → D:\user\Pictures，NVMe）

规模：源 86.8 万文件（多数 AppData 非媒体），目标库 15.5 万文件 / 697 GB。

**真跑 move 零风险**：同卷硬链接沙箱 `cp -al D:/old D:/perf/src; cp -al D:/user/Pictures D:/perf/out`（Git Bash 走 CreateHardLinkW），move 只改链接名；每轮重建 src；`rm -rf` 只删链接名。perf-collect 对 move 强制 `--dry-run`，而 dry-run 给每个待复制文件额外算 SHA-512，热点与真跑不同。

| 场景 | 优化前 | 优化后 |
|---|---|---|
| verify 1900 源 vs 目标库 | 32.4 s | 9.6 s（主循环并行；报告逐字段一致） |
| 非媒体为主 dry-run（77 万文件） | 2 min 08 s | 20–30 s（逐文件 warn 降 debug + 扩展名黑名单） |
| 真跑 move（硬链接沙箱） | 7 min 11 s | 6 min 53 s |

- verify 截距（1 源）3.6 s、斜率 16.6 ms/源（优化前）：截距 = 目标库建索引，斜率 = 逐源 SHA-512 + 候选比对
- 真跑 move 复制循环受 SHA-512 约束：重复文件双侧全量哈希（判重删源的安全前提，不可削弱）

## 环境要求

- **Linux**：`/usr/bin/time`（默认自带）
- **macOS**：`brew install gnu-time` 后 `gtime` 可用
- **Windows**：PowerShell 采样（50 ms 轮询）：wall clock / 峰值工作集（记入 `max_rss_kb`）/ 峰值私有内存 / User+System CPU / 退出码；无 page fault、上下文切换、fs IO 字段；tidymedia stderr 落 `tidymedia-stderr.log`
- **ONNX 真跑**：先用 `just build-release` 构建（`profile.release` 默认 opt=3，`perf-collect.ts` 亦显式设 env）；若用 `just OPT=0 build-release` 快速编译后真跑，推理慢 10-100 倍

## 已知限制

- **不产 CPU 采样火焰图**：AI 消费不到；如需函数级热点定位，另用 `samply record -- ./target/release/tidymedia ...` 后打开 profiler.firefox.com（人工分析路径）
- **不产 tracing span 时长**：`RUST_LOG=tidymedia=debug` 输出的现有 `debug!` 结构化日志已足够阶段级排查；专项 span 埋点 defer
- **远端 backend 内存放大**：`RemoteClient::read` 整文件入堆（smb2 `read_file`/adb_client API 限制），大视频在 Android/低内存主机 RSS 峰值不代表 CPU 侧算法内存

## 参考实现

- 采集脚本：`scripts/perf-collect.ts`（Bun+TypeScript，`bun scripts/perf-collect.ts`）；单测 `bun test tests/scripts`
- 耗时单点：`src/usecases/report.rs::elapsed_ms`（`coverage(off)` 免宿主时钟波动）
- 同步检查点：`CLAUDE.md`「新增 Report 时序/资源字段」
