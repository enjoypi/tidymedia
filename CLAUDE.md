# tidymedia 开发上下文

## 系统依赖
- 无外部进程依赖。EXIF/视频元数据走纯 Rust 库：`nom-exif`（图片+视频解析）+ `infer`（magic-bytes MIME）。
- Fixture 生成（开发时一次性，不在运行期依赖）用了 `ffmpeg` + `exiftool`：`sample-with-exif.jpg`、`sample-no-dates.jpg`、`sample-with-track.mp4`、`sample-no-track-date.mkv` 已 commit 到 `tests/data/`。
- nom-exif 内部用 `tracing::info!("find")` / `tracing::warn!("GPSInfo not found")` 大量输出，`install_logging` 必须用 EnvFilter 把 `nom_exif=error` 默认压住，保留 `RUST_LOG` 覆盖
- nom-exif 不 re-export chrono；测试构造 `EntryValue::DateTime/NaiveDateTime` 需把 `chrono` 加 dev-deps

## 测试与覆盖率
- 入口：`cargo nextest run`；默认覆盖率：`cargo llvm-cov nextest --summary-only`（stable，~99.6% region）
- **严格 100% 覆盖率（行/region/fn）**：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov nextest --summary-only`
  - 标了 `#[cfg_attr(coverage_nightly, coverage(off))]` 的函数会被 LLVM 跳过统计（不可稳定触发的 ? Err / expect panic / slice 边界伪 region）
  - `lib.rs` 和 `bin/tidymedia.rs` 顶部用 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]` 开启该 nightly feature
  - `Cargo.toml` 的 `[lints.rust] unexpected_cfgs` 已注册 `cfg(coverage_nightly)`，stable 编译无 warning
- `cargo llvm-cov nextest` 自动注入 `LLVM_PROFILE_FILE`，`assert_cmd` 子进程的覆盖率会被合并，因此 `main` 可被覆盖
- 定位未覆盖行：`cargo llvm-cov report --json` 后解析 `segments`（`count=0 && hasCount=true` 即 miss）；当 stats 报 miss 但 segments 全 covered，去看 `data[0].functions[*].regions` 内 `count=0`（macro/instantiation 级 region）
- `--ignore-run-fail` 与 `--no-fail-fast` 互斥，不能同时传
- 跨平台分支用 `#[cfg(target_os="windows")]` attribute，**不要**用 `cfg!()` 宏：后者在 Linux 上让 Windows 分支变成永远 false 的 missed region
- 测试函数内的 `?` 算作 region miss，测试签名用 `-> ()` + `.unwrap()` / `.expect()`，不要 `-> Result`
- IO Err 分支测试套路（实战已验证）：
  - **abs path 不存在** → `Info::from("/missing/abs/path")` 触发 metadata Err
  - **chmod 000 文件** → 触发 fs::File::open Err（unix-only，记得测试结束恢复权限避免 tempdir 清理失败）
  - **文件 mmap 前删除** → `Info::from(path).unwrap(); fs::remove_file(&path); info.calc_full_hash().unwrap_err();`
  - **自定义 fmt::Write always-Err** → 触发 Debug fmt 内 `writeln!(...)?` Err
  - **trace! 宏未启用导致 region miss** → 测试用 `tracing::subscriber::with_default(...)` 注入 trace-level subscriber 让闭包被求值
  - **EXIF/track 的 None 分支** → 用"无日期标签的 JPEG / 无 CreateDate 的 MKV"两个 fixture 让 `parsed.get(...)` 返回 None
  - **parse_exif 的 Err 分支** → visit_dir 之后立刻 `fs::remove_file` 删源，`Exif::from_path` metadata 失败
  - **`open_read` 成功但 stream Err** → `FakeBackend::inject_reader_error(loc, kind)` 注入 read 立即 Err 的 reader，专门触发 `fast_hash_stream` / `sniff_mime` 等调用点的 `?` Err（替代原 `fast_hash`/`full_hash`/`secure_hash` 的 path 版 `coverage(off)`，stream 版默认 100% 覆盖）
- **expect/unwrap 的 panic 边永远算 region miss**：不可通过测试 cover，要么抽 helper 单独标 `coverage(off)`，要么直接接受
- 消除生产代码 `?` Err 不可触发分支的优先级：①改 `unwrap_or` 兜底 → ②返 `Option` 替代 `Result`（小重构）→ ③才考虑 `#[cfg_attr(coverage_nightly, coverage(off))]`。前两者让 stable 默认就 100%
- `std::env::set_var` 在 Rust 1.75+ 必须包 `unsafe { }`（与 edition 无关）；nextest 进程隔离让其可安全用于单测

## Fixture
- `tests/data/` 下文件的 mtime 每次 `git checkout` 都会被重置，EXIF / 时间相关测试必须用 `filetime::set_file_mtime` 固定
- 已封装：`entities/test_common::copy_png_to(dir, name)` 复制 PNG 并把 mtime 固定到 `FIXED_MEDIA_MTIME`（2024-01-01 12:00:00 UTC）
- `camino::Utf8Path` 在 Linux 上**不**把 `\` 当分隔符，原有 Windows 反斜杠路径测试在 Linux 上行为不同
- ffmpeg 生成实测：`color=s=0x0` 无效（用 `8x8`）；MP4 不传 `-metadata creation_time=` 时 nom-exif 返回 `Some(1904-01-01)`（QuickTime epoch），要 None 用 MKV

## 文件组织
- 单文件 > 512 行时拆测试：`#[cfg(test)] #[path = "X_tests.rs"] mod tests;`（保留 `super::` 路径关系）
- `entities/test_common` 与 `entities/exif` 是 `pub(crate)`，跨模块测试可访问

## 项目分层（Clean Architecture）
- 三层（自外向内）：`src/bin/tidymedia.rs`（Frameworks）→ `src/lib.rs`（Interface Adapter / CLI）→ `src/usecases/`（Use Cases）→ `src/entities/`（Entities）
- `bin/tidymedia.rs` **只**调 `tidymedia::run_cli(env::args_os())`，零业务逻辑，所有可测代码上移到 `lib.rs`
- `lib.rs` 持有 `Cli`/`Commands` 与 `tidy()` 调度；clap 解析、日志初始化、命令分发都在这层
- `usecases/` 仅依赖 `entities/`，对外通过 `mod.rs` 用 `pub(super)` 暴露 `copy` / `find_duplicates`；不直接面向 CLI 参数结构
- `entities/backend/` 是 Gateway 抽象：`trait Backend` + `Local / Smb / Mtp` 三实现 + 测试 `FakeBackend`；`file_info` / `file_index` / `exif` / `sidecar` 都 backend-aware（持 `Arc<dyn Backend>`，旧 `Info::from(&str)` / `Index::new()` / `Exif::from_path_with_offset` / `sidecar::discover` 均退化为 LocalBackend shim）
- 目录名是 `usecases`（无下划线），跨层导入用 `crate::usecases::...` / `crate::entities::...`

## URI 与 Backend
- CLI `sources` / `output` 接 `Location`（实现 `FromStr` 让 clap 自动 value_parser）：
  - 无 `://` 或 `local://` ⇒ `Location::Local`
  - `smb://[user@]host[:port]/share/path` ⇒ `Location::Smb`
  - `mtp://device/storage/path` ⇒ `Location::Mtp`
  - 字段内空格 / 中文 / 路径分隔符走 `percent-encoding`，**不引 `url` crate**（`entities/uri.rs` 自实现解析）
- SMB 凭据：`SMB_USER` 经配置 `backend.smb.default_user` 兜底；`SMB_PASSWORD` 由 `SmbTarget::password` 在 `build_target` 处读 env；Kerberos 走 `KRB5CCNAME`。**密码永远不入 YAML**（CLAUDE.md P0.13）
- 当前 `SmbBackend::new()` / `MtpBackend::new()` 返 `io::ErrorKind::Unsupported` "not enabled"；测试入口是 `SmbBackend::with_client(Arc<dyn SmbClient>)` / `MtpBackend::with_client(...)`；真实 `RealSmbClient` / `RealMtpClient` 适配器留作未来 PR
- `tidy()` 内 `require_local_path` adapter：Local 透传，Smb/Mtp 报清晰 "<scheme> backend not enabled in this build; rebuild with --features <scheme>-backend"
- Registry / `for_scheme` 路由暂未实装：现行仅 Local 路径走 use case；Smb/Mtp 在 Cli adapter 处被拒收

## SMB/MTP 测试套路
- **手写 FakeSmbClient / FakeMtpClient**：state 用 `Arc<Mutex<HashMap<...>>>`；`inject(SmbOp::Read, path, ErrorKind::TimedOut)` 注入逐 op + 逐 path 的错误，无须 `mockall`
- **EACCES 映射**：`map_smb_error` 对 `io::Error::other` + 文案含 `"EACCES"` 转 `PermissionDenied`；FakeSmbClient 用同一文案触发
- **env 凭据传递**：`build_target` 读 `SMB_PASSWORD` / `KRB5CCNAME`；测试用 `unsafe { std::env::set_var(...) }` + nextest 进程隔离让其安全（与 `OnceLock` / `expand_env` 测试同套路，见 P0.4 与 CLAUDE.md「工具链注意」）
- **真实 client 适配器**：未来 `RealSmbClient::{stat, list, read, write, unlink, mkdir}` / `RealMtpClient::{...}` 各函数标 `#[cfg_attr(coverage_nightly, coverage(off))]`；调度逻辑（`build_target` / `parent_target` / `map_smb_error` / `SmbBufferedWriter`）已 100% 覆盖
- **Backend trait 方法的 rejection 测试**：每个方法测三类输入——OK / client Err 注入 / 非自家 scheme 返回 `InvalidInput`
- **FakeBackend `inject_reader_error`**：让 `open_read` 成功但返回的 reader 在 `read` 时立即 Err，专门覆盖调用方在 stream hash / `sniff_mime` 等位置的 `?` Err 分支

## 配置与日志
- 运行时配置：`config.yaml`（项目根）+ `src/usecases/config.rs`，`config()` 返回 `&'static Config`（`OnceLock`）
- 切换配置：`TIDYMEDIA_CONFIG=/path/to.yaml`；语法 `${VAR:-default}` 由 `expand_env` 自实现（不引 dotenv）
- `FAST_READ_SIZE` 因 `[0; FAST_READ_SIZE]` 栈数组要求编译期常量，**不外置**（R1 合理例外）
- 结构化日志字段约定：`feature` / `operation` / `result`（CLI 工具无 request_id/user_id）
- `UtcOffset::from_whole_seconds` 范围 ±25:59:59，越界返回 `None`，用 `.unwrap_or(UtcOffset::UTC)` 兜底
- **R1 外置范围**：`copy.timezone_offset_hours` / `copy.unique_name_max_attempts` / `exif.valid_date_time_secs` + `backend.smb.{default_user,timeout_secs}` / `backend.mtp.{device_match,storage_match}` 需运维可调。其余 const 属 R1 边界例外**不外置**：
  - **spec §X 算法常量**：`EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS` / `MTIME_VS_P0_HINT_SECS`（filter/resolve）
  - **协议字面量**：`PHONE_PREFIX="IMG_"` / `CAMERA_PREFIX="DSC_"` / `SCREENSHOT_PREFIX="Screenshot_"` / `XMP_KEY` / `META_TYPE_IMAGE` / `META_TYPE_VIDEO`
  - **日志维度名**：`FEATURE_CLI` / `FEATURE_COPY` / `FEATURE_FIND` / `FEATURE_INDEX`
  - **lookup 表**：`MONTH: [&str; 13]`（copy.rs 月份零填充表）
  - **流式哈希**：`FAST_READ_SIZE`（栈数组要求编译期常量）/ `STREAM_CHUNK = 1 MiB`（远端 backend syscall 频率 vs 网络往返平衡）/ `MIME_SNIFF_BYTES = 256`（`infer::get` 仅看前 16-32 字节）
- `src/usecases/copy.rs` 的 `println!("\"{}\"\t\"{}\"", src, dst)` 是 CLI 脚本可读输出（dry-run + 完成回执），**不是** R3 日志路径，不要改成 tracing

## 工具链注意
- nextest 每个测试独立进程，`set_var`/`remove_var`/`OnceLock` 不会跨测试污染（区别于 `cargo test`）
- 仓库 baseline 已有 clippy errors（`io_other_error` 等），改动前先 `git stash` 跑 baseline 再对照
- HashMap 并行 in-place 改 value：用 `self.files.par_iter_mut().for_each(|(k, v)| ...)`，避免"par_iter→Vec→再 get_mut Option None"的不可达分支
