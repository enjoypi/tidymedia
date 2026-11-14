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

采集前必须用 `just build-release` 重新构建 opt=3 产物；脚本只检查二进制存在，无法识别 `OPT=0` 产物。

```bash
bun scripts/perf-collect.ts --sub <copy|move|find|move-text-shot|verify> --data <真实源目录> --output-dir <目录>
```

- 除 `find` 外所有子命令必须提供 `--output-target <目标库>`；额外 CLI 参数用 `--extra`。
- copy/move/move-text-shot 默认 dry-run，其测量热点不一定等同正式移动。
- 产物包括 `report.json`、`time-v.txt`、`perf-report.md`；Windows 另保留 stderr 日志。产物保持机器可读，不生成 SVG 火焰图或二进制 pprof。
- 修改参数或产物字段时同步脚本测试和本文件。

## 指标与分析

- 报告不含执行模式和构建方式。交付分析时人工注明命令、数据规模、文件类型、dry-run 或真跑、操作系统和构建方式；结论引用具体指标，不只凭总耗时判断。
- `duration_ms` 是 use case 内耗时，不包含进程启动和模型加载；整进程 wall clock 才包含固定开销。
- macOS 使用 `gtime -v`（GNU time）采集 wall clock 与内存指标；未安装时先执行 `brew install gnu-time`。
- Windows 使用 PowerShell 采样，提供 wall clock、CPU、峰值工作集和峰值私有内存；没有 Unix page fault、上下文切换和 fs IO 字段时不得编造。
- Windows 工作集包含文件缓存页，不等于堆占用；判断内存压力优先看峰值私有内存，并结合文件大小和读取方式。
- CPU 空闲且吞吐低时先排查 IO 和重复读取；CPU 饱和时再检查内容判重或推理。system CPU 偏高时检查频繁 metadata/open/close。
- wall clock 明显大于 `duration_ms` 时检查启动和模型加载；只有实际使用模型的命令才归因模型开销。
- major page fault 或私有内存异常增长时检查并发、整文件读取和缓冲；不得通过削弱内容判等换取表面性能。
- 逐文件日志造成的瓶颈先核对日志级别和汇总策略，不把一次实测秒数写成通用阈值。
