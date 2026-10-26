# 构建与覆盖率工具规则

## justfile 与 MSVC

- justfile 是日常 build/test/lint/cov/gate 的统一入口。日常 build/test/lint/cov 使用 nightly + dev + default features；普通 target 与 coverage target 保持隔离。
- justfile 是 gate 步骤的唯一权威；cov 已含全量 nextest，不追加重复测试，coverage 必须独占运行。文档不复制步骤序列。
- Windows 全部 Rust 工具链使用 `x86_64-pc-windows-msvc`，链接配置固定在 `.cargo/config.toml`。
- Windows 编译命令必须在同一 `cmd /c` 进程中执行 `call tools\msvc-env.cmd && <命令>`，使 vcvars 环境作用于后续命令；不得独立执行 `.cmd` 后指望父 Bash 继承环境。
- 本机 clean environment 下必须确保 System32、可写 TMP/TEMP 和 cargo bin 可用，个人临时目录不写入共享配置。System32 优先从 `SystemRoot` 构造；`.cargo/config.toml` 的固化链接/LIB 与 `msvc-env.cmd` 的动态发现当前并存，升级工具链时必须同时核对，避免版本漂移。
- macOS 没有 GNU timeout；预计超过 2 分钟的命令使用临时包装 `uv run --quiet --no-project python -c 'import signal,os,sys; signal.alarm(110); os.execvp(sys.argv[1], sys.argv[1:])' <cmd> <args>` 截断并步进推进，被杀轮与测试失败退出码分别判断；该包装不是仓内脚本。
- 新增或修改用户命令时更新根 `CLAUDE.md` 的入口；行为以 justfile 为准，本文件不复制 recipe 步骤。

## 覆盖率实现

- `tools/cov.sh` 是唯一覆盖率门禁实现，顺序为 branch → region/function/line，并执行 lcov 完整性检查。Windows 与 macOS 双平台各自运行门禁，均需达到 branch、region、function、line 100%。
- Windows + nightly 使用 `--no-rustc-wrapper`，通过 RUSTFLAGS 注入覆盖率，避免 RUSTC_WRAPPER 导致静默未插桩。
- Windows cov 必须先导入 MSVC 环境；追加 coverage rustc cfg 时保留 `/LIBPATH`，确保原生依赖可链接。
- lcov `SF:` 路径兼容 `/` 与 `\`；ignore-regex 使用 `[/\\]`，完整性自检先归一化为相对正斜杠路径。
- 覆盖率排除现状包括真实 backend、真实模型入口、模型主体和 per-instance `_phantom.rs`；最后一类可含实际业务逻辑，不得误认为仅限模型。新增排除必须说明无法测试的原因，不得用于隐藏可测分支。
- 日常源码变更后只清理 profraw，保留插桩编译缓存；修改 Cargo.toml、coverage 属性或工具配置后才清理 workspace 插桩产物。
- macOS cov 为完整门禁，与 Windows 同一口径，不再视为固定失败。
