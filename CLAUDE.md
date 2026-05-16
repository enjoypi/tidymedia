# tidymedia 开发上下文

## 系统依赖
- EXIF 解析依赖外部 `exiftool` 命令：`sudo apt-get install -y libimage-exiftool-perl`（Cargo.toml 看不出）

## 测试与覆盖率
- 入口：`cargo nextest run`；覆盖率：`cargo llvm-cov nextest --summary-only`
- `cargo llvm-cov nextest` 自动注入 `LLVM_PROFILE_FILE`，`assert_cmd` 子进程的覆盖率会被合并，因此 `main` 可被覆盖
- 定位未覆盖行：`cargo llvm-cov report --json` 后解析 `segments`（`count=0 && hasCount=true` 即 miss），不要依赖 `--show-missing-lines`（0.8.x 不稳定）
- `--ignore-run-fail` 与 `--no-fail-fast` 互斥，不能同时传
- 跨平台分支用 `#[cfg(target_os="windows")]` attribute，**不要**用 `cfg!()` 宏：后者在 Linux 上让 Windows 分支变成永远 false 的 missed region
- 测试函数内的 `?` 算作 region miss，测试签名用 `-> ()` + `.unwrap()` / `.expect()`，不要 `-> Result`

## Fixture
- `tests/data/` 下文件的 mtime 每次 `git checkout` 都会被重置，EXIF / 时间相关测试必须用 `filetime::set_file_mtime` 固定
- 已封装：`entities/test_common::copy_png_to(dir, name)` 复制 PNG 并把 mtime 固定到 `FIXED_MEDIA_MTIME`（2024-01-01 12:00:00 UTC）
- `camino::Utf8Path` 在 Linux 上**不**把 `\` 当分隔符，原有 Windows 反斜杠路径测试在 Linux 上行为不同

## 文件组织
- 单文件 > 512 行时拆测试：`#[cfg(test)] #[path = "X_tests.rs"] mod tests;`（保留 `super::` 路径关系）
- `entities/test_common` 与 `entities/exif` 是 `pub(crate)`，跨模块测试可访问

## 配置与日志
- 运行时配置：`config.yaml`（项目根）+ `src/use_cases/config.rs`，`config()` 返回 `&'static Config`（`OnceLock`）
- 切换配置：`TIDYMEDIA_CONFIG=/path/to.yaml`；语法 `${VAR:-default}` 由 `expand_env` 自实现（不引 dotenv）
- `FAST_READ_SIZE` 因 `[0; FAST_READ_SIZE]` 栈数组要求编译期常量，**不外置**（R1 合理例外）
- 结构化日志字段约定：`feature` / `operation` / `result`（CLI 工具无 request_id/user_id）
- `UtcOffset::from_whole_seconds` 范围 ±25:59:59，越界返回 `None`，用 `.unwrap_or(UtcOffset::UTC)` 兜底

## 工具链注意
- nextest 每个测试独立进程，`set_var`/`remove_var`/`OnceLock` 不会跨测试污染（区别于 `cargo test`）
- 仓库 baseline 已有 clippy errors（`io_other_error` 等），改动前先 `git stash` 跑 baseline 再对照
