# 脚本与性能采集规则

## 脚本维护

- 脚本语言见根 `CLAUDE.md`。独立脚本用 `bun <script>` 执行，测试放 `tests/scripts`，用 `bun test tests/scripts` 运行。
- 批量文本修改后核对预期数量和实际结果。含 Windows 反斜杠的内容使用 Edit/Write，避免 shell heredoc 或内联表达式吞掉转义。
- `find` 生成的删除脚本必须转义反斜杠、引号和控制字符；保护项保留注释和明确 SURVIVOR 标记，不能生成会误删最后副本的默认操作。

## 模型准备

- 分类模型（bge-small-zh-v1.5 int8 与 tokenizer）以 git-lfs 入库在 `models/`；`bun scripts/download_models.ts` 仅用于重新下载。OCR 模型由用户自备，路径见 `config.yaml`。
- HuggingFace 不可达时，可临时设置 `HF_ENDPOINT=https://hf-mirror.com`，不把镜像写成固定依赖。
- 修改模型后同步输入 shape、decode、配置路径、Fake 和真实模型 smoke test；模型规格与 tract 运行约束见 `src/CLAUDE.md`。

## 性能采集

采集前必须用 `just build-release` 重新构建 opt=3 产物；脚本会拒绝早于源码的二进制，但无法识别 `OPT=0` 产物。

```bash
bun scripts/perf-collect.ts --sub <copy|move|copy-doc|move-doc|find|move-text-shot|verify> --data <真实源目录> --output-dir <目录>
bun scripts/perf-compare.ts <基线产物目录> <候选产物目录>
bun scripts/perf-dataset.ts <clone|photos|stress> --to <目录>
```

- 除 `find` 外所有子命令必须提供 `--output-target <目标库>`；额外 CLI 参数用 `--extra`。输出目录和目标库位于 `--data` 之下时拒绝运行。
- copy/move/copy-doc/move-doc/move-text-shot 默认 dry-run，其测量热点不一定等同正式移动。
- `--binary` 指定另存的二进制（用于基线与候选交替对比），此时不做新旧检查。
- `--repeat`、`--warmup` 控制测量与预热次数；`--max-private-mib` 为峰值私有内存门禁，超限退出码 3，采不到指标退出码 4。
- 产物为 `perf.json`（命令、数据规模、git、各次指标、中位数与极值）和 `runs/<i>/{report.json,time.txt,stdout.txt}`。产物保持机器可读，不生成 SVG 火焰图或二进制 pprof。
- `perf-compare` 仅在两组分布不重叠时判改进或回归，其余判噪声；stdout 或报告计数不一致时非零退出。实测秒数只留在产物中，不写入测试、文档或阈值。
- `perf-dataset` 生成不入库数据集：`clone` 克隆真实目录（`--mutate-tail` 让同名文件内容不同），`photos` 以 `tests/data` 为种子生成大量小文件，`stress` 生成大像素图片及输出库同名副本。
- 修改参数或产物字段时同步脚本测试和本文件。

## 指标与分析

- `perf.json` 不含构建方式。交付分析时人工注明构建方式和数据覆盖范围（如是否含照片）；结论引用具体指标，不只凭总耗时判断。
- `duration_ms` 是 use case 内耗时，不包含进程启动和模型加载；整进程 wall clock 才包含固定开销。
- macOS 使用系统自带 `/usr/bin/time -l`，峰值私有内存取内核记录的 peak memory footprint，不采样、不依赖 gtime。
- Windows 使用 PowerShell 采样，提供 wall clock、CPU、峰值工作集和峰值私有内存（`PeakPagedMemorySize64`）；没有 Unix page fault、上下文切换和 fs IO 字段时不得编造。
- Windows 工作集包含文件缓存页，不等于堆占用；判断内存压力优先看峰值私有内存，并结合文件大小和读取方式。
- CPU 空闲且吞吐低时先排查 IO 和重复读取；CPU 饱和时再检查内容判重或推理。system CPU 偏高时检查频繁 metadata/open/close。
- wall clock 明显大于 `duration_ms` 时检查启动和模型加载；只有实际使用模型的命令才归因模型开销。
- major page fault 或私有内存异常增长时检查并发、整文件读取和缓冲；不得通过削弱内容判等换取表面性能。
- 逐文件日志造成的瓶颈先核对日志级别和汇总策略，不把一次实测秒数写成通用阈值。
