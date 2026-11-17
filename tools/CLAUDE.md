# 构建与覆盖率工具规则

## justfile 与 MSVC

- 命令入口见根 `CLAUDE.md`，recipe 步骤以 justfile 为准，文档不复制。新增或修改命令时更新根 `CLAUDE.md`。
- `rust-toolchain.toml` 把项目默认工具链固定为 rolling `nightly`（components：rustfmt、clippy、llvm-tools），禁止日期版；fmt/check/lint/build/test/cov 显式 `+nightly`，`build-release` 显式 `+stable`。
- 日常 build/test/lint/cov 使用 nightly + dev；普通 target 与 coverage target 保持隔离。cov 已含全量 nextest，coverage 必须独占运行。
- build/check/lint/test 共用同一 RUSTFLAGS 与 dev profile，第三方依赖产物完全共享；coverage 因插桩标志必须独立 target，不与 dev 产物混用。
- Windows 全部 Rust 工具链使用 `x86_64-pc-windows-msvc`，编译统一经 justfile 的 `[windows]` recipe。链接器、LIB 与 `/LIBPATH` 只由 `tools/msvc-env.cmd` 动态发现，升级 MSVC/SDK 无需手改配置。
- Windows 编译命令必须在同一 `cmd /c` 进程中执行 `call tools\msvc-env.cmd && <命令>`；独立执行 `.cmd` 不会把环境带回父进程。
- Windows clean environment 下必须确保 System32（优先从 `SystemRoot` 构造）、可写 TMP/TEMP 和 cargo bin 可用；个人临时目录不写入共享配置。

## 覆盖率实现

- `tools/cov.ts` 是唯一覆盖率门禁实现。一次 `llvm-cov report` 同时产出 lcov 并携带 `--fail-under-file-lines/functions/lines/regions` 阈值；随后按 branch → line → 生产文件完整性顺序解析 lcov，最后并入阈值退出码。Windows 与 macOS 同一口径，各自运行，四项均为 100%。
- `clean`、`report`、`nextest` 三类 cargo 调用一律带 `--locked`。缺 `--locked` 的 `llvm-cov clean`/`report` 会在门禁运行中静默重解析依赖图并改写 `Cargo.lock`（实测一次改写 65 处 semver 范围内版本）。
- lcov 落地后立即再执行一次 `clean --profraw-only` 删除 profraw；插桩编译缓存保留复用。`target/llvm-cov-target/*.profdata` 是事后定位 miss 的唯一凭据，删 profraw 不影响它。
- 跑测器保持 nextest（每测试独立进程）。已实测否决 `cargo test`：bulk 模式下 lib 有 174〜220 个测试失败，160 个 panic 落在 `usecases::config.rs` 的 `config()`，根因是配置经 `once` + `Box::leak` 成为进程级单例，首个加载者定型且顺序不定；即使跳过全部改 env/cwd/卷的模块也不能收敛。要改必须先拆掉该单例。
- 覆盖率排除只使用 `tools/cov.ts` 的 `COV_NONBUSINESS` 与 `COV_TEST_SUPPORT` 逐文件精确清单，文件必须只含独立非业务边界或测试支持；不得按目录或 `tract_*`、`_real`、`_io` 等后缀排除，禁止 `coverage(off)`。
- `COV_ZERO_IGNORE=1 just cov` 用于零忽略审计；该模式不得传空 ignore regex，报告与完整性自检都必须绕开排除。最终业务门禁仍使用 `just cov`。
- 门禁的纯逻辑由 `tests/scripts/cov.test.ts` 覆盖；修改 `tools/cov.ts` 先改测试。
- Windows + nightly 使用 `--no-rustc-wrapper`，通过 RUSTFLAGS 注入覆盖率，避免 RUSTC_WRAPPER 导致静默未插桩。追加 coverage rustc cfg 时保留 `/LIBPATH`，确保原生依赖可链接。
- lcov `SF:` 路径兼容 `/` 与 `\`；ignore-regex 使用 `[/\\]`，完整性自检先归一化为相对正斜杠路径。
- 日常源码变更后只清理 profraw，保留插桩编译缓存；修改 Cargo.toml、coverage 属性或工具配置后才清理 workspace 插桩产物。
