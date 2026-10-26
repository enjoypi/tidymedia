# 脚本与性能采集规则

## 脚本维护

- 新增通用自动化脚本使用 Bun + TypeScript；既有平台包装和模型准备脚本可保留 Shell/Python。独立 Bun 脚本用 `bun <script>` 执行，测试放在对应测试目录。
- 批量文本修改后核对预期数量和实际结果。含 Windows 反斜杠的内容使用 Edit/Write，避免 shell heredoc 或内联表达式吞掉转义。
- 生成或维护 Python 输出时，Windows stdout 需要 UTF-8 和 LF 须显式配置，中文输出优先写 UTF-8 文件再读取；新自动化脚本仍使用 Bun + TypeScript。
- 删除脚本必须转义反斜杠、引号和控制字符；保护项保留注释和明确 SURVIVOR 标记，不能生成会误删最后副本的默认操作。

## 模型准备

- ONNX 模型通过 git-lfs 管理，装配入口为 `scripts/download_models.sh`；模型路径为空时，调用方应立即返回 InvalidInput。
- HuggingFace 大文件不可达时，可将 `HF_ENDPOINT=https://hf-mirror.com` 作为可选故障排查手段，不把镜像写成固定依赖。
- 修改模型后同步输入 shape、decode、配置路径、Fake 和真实模型 smoke test；模型规格与 tract 运行约束见 `src/CLAUDE.md`。

## 性能采集

采集前必须用 `just build-release` 重新构建 release opt=3 产物。脚本只检查二进制存在，不能识别先前的 `just OPT=0 build-release`；运行脚本时设置优化环境变量也不会改变已有二进制。

```bash
bun scripts/perf-collect.ts --sub <copy|move|find|move-text-shot|verify> --data <真实源目录> --output-dir <目录>
```

- `verify` 必须增加 `--output-target <目标库>`；需要额外 CLI 参数时使用脚本提供的 `--extra`。
- copy/move/move-text-shot 默认 dry-run；其测量热点不一定等同正式移动。脚本目前不会自动在 `perf-report.md` 标明执行模式或构建方式，交付分析时必须人工补充，不得把运行时环境变量当作构建证明。
- 产物包括 `report.json`、`time-v.txt`、`perf-report.md`；Windows 另保留 stderr 日志。产物保持机器可读，不生成 SVG 火焰图或二进制 pprof 作为默认交付。
- 采集脚本测试使用 `bun test tests/scripts`；修改参数或产物字段时同步脚本测试和本文件。

## 指标与分析

- `duration_ms` 是 use case 内耗时，不包含进程启动和模型加载；整进程 wall clock 才包含固定开销。
- macOS 使用 `gtime -v`（GNU time）采集 wall clock 与内存指标；未安装时先执行 `brew install gnu-time`。
- Windows 使用 PowerShell 采样，提供 wall clock、CPU、峰值工作集和峰值私有内存；没有 Unix page fault、上下文切换和 fs IO 字段时不得编造。
- Windows 工作集包含本地映射文件页，不等于堆占用；判断内存压力优先看峰值私有内存，并结合文件大小和读取方式。
- CPU 空闲且吞吐低时先排查 IO 和重复读取；CPU 饱和时再检查内容判重或推理。system CPU 偏高时检查频繁 metadata/open/close。
- wall clock 明显大于 `duration_ms` 时检查启动和模型加载；只有实际使用模型的命令才归因模型开销。
- major page fault 或私有内存异常增长时检查并发、整文件读取和缓冲；不得通过削弱内容判等换取表面性能。
- 逐文件日志造成的瓶颈先核对日志级别和汇总策略，不把一次实测秒数写成通用阈值。

向 AI 提供 `perf-report.md`、`report.json` 和 `time-v.txt`，并注明命令、数据规模、文件类型、dry-run/真跑、操作系统及发布构建方式；要求结论引用具体指标，不只凭总耗时判断。
