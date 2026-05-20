@~/.claude/CLAUDE.md

@~/.claude/rust.md

# tidymedia 开发上下文

## Quick Start
- 构建：`cargo build`；运行：`cargo run -- copy /source -o /output`；dry-run：`cargo run -- copy /source -o /output --dry-run`
- 测试：`cargo nextest run`；覆盖率：`cargo llvm-cov nextest --summary-only`
- lint：`cargo +nightly fmt && cargo clippy`

## 系统依赖
- 无外部进程依赖。EXIF/视频元数据走纯 Rust 库：`nom-exif`（图片+视频解析）+ `infer`（magic-bytes MIME）。
- Fixture 生成（开发时一次性，不在运行期依赖）用了 `ffmpeg` + `exiftool`：`sample-with-exif.jpg`、`sample-no-dates.jpg`、`sample-with-track.mp4`、`sample-no-track-date.mkv` 已 commit 到 `tests/data/`。
- nom-exif 内部用 `tracing::info!("find")` / `tracing::warn!("GPSInfo not found")` 大量输出，`install_logging` 必须用 EnvFilter 把 `nom_exif=error` 默认压住，保留 `RUST_LOG` 覆盖
- nom-exif 不 re-export chrono；测试构造 `EntryValue::DateTime/NaiveDateTime` 需把 `chrono` 加 dev-deps

## 测试与覆盖率
- 入口：`cargo nextest run`；默认覆盖率：`cargo llvm-cov nextest --summary-only`（stable，~99.6% region）
- `cargo nextest run` 无 `--quiet` flag；静默输出用 `2>&1 | tail -N` 或 nextest 自己的 `--status-level` / `--failure-output`
- **严格 100% 覆盖率（行/region/fn/branch）**：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov nextest --summary-only [--branch]`
  - 标了 `#[cfg_attr(coverage_nightly, coverage(off))]` 的函数会被 LLVM 跳过统计（不可稳定触发的 ? Err / expect panic / slice 边界伪 region）
  - `lib.rs` 和 `bin/tidymedia.rs` 顶部用 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]` 开启该 nightly feature
  - `Cargo.toml` 的 `[lints.rust] unexpected_cfgs` 已注册 `cfg(coverage_nightly)`，stable 编译无 warning
  - **`--branch` 的 multi-binary 多 instance 陷阱**：lib unit + 集成 test binary 各自 codegen Info::open / calc_full_hash / do_copy 等热点 fn 的副本，每个 binary 的 fn instance 都有独立的 `[True, False]` 计数器；如果某个 instance（如 lib_tidy 集成 binary）从未触发某 boundary case（dir/empty file/cache hit/duplicate+dry_run 不同组合），LLVM 即报 instance-level miss，**`#[inline(never)]` 与 `[profile.test] codegen-units=1` 均无效**；`if let Some(_) = helper()` 把 cache check 拆 helper 也不行（主 fn 仍有 if-let branch）。可行路径只有：①重构成 `?` 风格（`?` 算 region 不算 branch，且 helper 标 `coverage(off)` 能透传 hide 内部 branch，例：`Info::open` 拆 `ensure_hashable(&meta, loc)?`）；②函数级 `coverage(off)`（已用在 `calc_full_hash` / `secure_hash` / `create_time` / `PartialEq::eq` / `full_path` / `do_copy`，语义由独立单元/集成测试断言不退化）
- `cargo +nightly llvm-cov nextest --summary-only --branch`（unstable flag）跑 branch 覆盖率；定位 miss 用 `--text --output-path /tmp/cov.txt` 后 `awk '/<file>\.rs:$/,/^\/home.*<next>\.rs:$/' /tmp/cov.txt | grep -B1 "True: 0, False: [1-9]"`，或 `--json --output-path /tmp/cov.json` 后解析 `f["branches"]`（每条 `[line, col, _, _, T, F, file_id, expanded, kind]`）
- **改 `Cargo.toml` / `coverage` 属性后必跑 `cargo +nightly llvm-cov clean --workspace`**，否则 `report --branch` 仍读 stale `.profraw`，看到的 missed 数字与实际不符
- clippy baseline 已有 12 warnings（`needless_borrows_for_generic_args` 等，含 `lib test` 9 unique），**不要**带 `-D warnings` 跑——baseline warning 会被当作回归。用 `cargo clippy --all-targets`（无 deny），git stash 对照 my 改动是否新增 warning 即可
- `camino::Utf8PathBuf::from("file").parent()` 返 `Some("")` 而**非** `None`（与 `std::path::Path` 一致）；只有 `Utf8PathBuf::from("").parent()` 才是 `None`。写"path 单 component 触发 parent==空"边界测试要用 `"file.txt"` 不是 `""`
- `#[cfg_attr(coverage_nightly, coverage(off))]` 可加在 **trait `impl` 内 method**（如 `impl PartialEq for Info { #[cfg_attr(...)] fn eq(...) }`），不必标在整个 `impl` 块上——单 method off 不影响同 impl 块其他 method 统计
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
- CA 重构移动文件时：`pub use A::B` 只 re-export 类型不保留路径，`pub mod B { pub use A::B::*; }` 保留完整模块路径；配套测试的 `super::` 需改为 `crate::` 绝对路径
- CA 依赖方向验证：`grep -rn "use crate::adapters\|use crate::frameworks" src/entities/ src/usecases/` 应仅返回 re-export 桥接，不含业务逻辑导入

## 项目分层（Clean Architecture）
- 四层（自外向内）：`src/frameworks/`（Frameworks）→ `src/adapters/`（Interface Adapters）→ `src/usecases/`（Use Cases）→ `src/entities/`（Entities）
- `bin/tidymedia.rs` **只**调 `tidymedia::run_cli(env::args_os())`，零业务逻辑
- `lib.rs` 仅做模块声明 + re-export，不含业务逻辑
- `adapters/` 持有 CLI 解析（`cli.rs`）、命令调度（`dispatch.rs`）、Gateway 实现（`backend/`：`local.rs` / `remote.rs` / `smb.rs` / `adb.rs` / `mtp.rs` / `fake.rs` / `fake_remote.rs` + 对应 `*_real.rs` / `*_tests.rs`）、Backend 工厂（`backend/factory.rs`）
- `frameworks/` 持有配置加载 IO（`config.rs`：`OnceLock` + `config()` + `load()` + `expand_env()`）
- `usecases/` 仅依赖 `entities/`，对外通过 `mod.rs` 用 `pub(super)` 暴露 `copy` / `find_duplicates`；配置结构体定义留在 `usecases/config.rs`，通过 re-export `pub use crate::frameworks::config::config;` 让 usecases 内部用 `super::config::config` 不直接依赖 frameworks
- `entities/backend/` 是 Gateway 抽象：`trait Backend` + 值类型（`SmbTarget` / `AdbTarget` / `MtpTarget`）；具体实现通过 re-export 模块（`pub mod local { pub use crate::adapters::backend::local::LocalBackend; }` 等）保持原有路径；`file_info` / `file_index` / `exif` / `media_time::sidecar` 都 backend-aware（持 `Arc<dyn Backend>`）
- `entities/common.rs` 是非测试用途的共享工具（`test_common.rs` 是测试专用）
- 目录名是 `usecases`（无下划线），跨层导入用 `crate::usecases::...` / `crate::entities::...` / `crate::adapters::...` / `crate::frameworks::...`

## URI 与 Backend

### URI 格式
- CLI `sources` / `output` 接 `Location`（实现 `FromStr` 让 clap 自动 value_parser）：
  - 无 `://` 或 `local://` ⇒ `Location::Local`
  - `smb://[user@]host[:port]/share/path` ⇒ `Location::Smb`
  - `mtp://device/storage/path` ⇒ `Location::Mtp`
  - `adb://[serial]/abs/path` ⇒ `Location::Adb`；serial 为空（`adb:///sdcard/...`）让 client autodetect 唯一在线设备；path 始终是设备上绝对路径（以 `/` 开头）
  - 字段内空格 / 中文 / 路径分隔符走 `percent-encoding`，**不引 `url` crate**（`entities/uri.rs` 自实现解析）
- 任意混合 sources 已支持：`copy smb://a /local/b mtp://c adb:///sdcard/d -o /x` 合法；[`Index`] 内部每条 `Info` 自带 `Arc<dyn Backend>`，`visit_location(&Location, Arc<dyn Backend>)` 显式接 backend 入参

### 凭据
- SMB：`SMB_USER` 经配置 `backend.smb.default_user` 兜底；`SMB_PASSWORD` 由 `SmbTarget::password` 在 `build_target` 处读 env；Kerberos 走 `KRB5CCNAME`。`backend.smb.workgroup` 默认 `WORKGROUP`，pavao `SmbCredentials::workgroup` 必填。**密码永远不入 YAML**（CLAUDE.md P0.13）
- ADB：走本机 `adb` daemon 协议（adb_client 3.2 通过 TCP 连接 `127.0.0.1:5037`）；运行前需 `adb start-server`、Android 设备开 USB 调试 + 文件传输模式；多设备时 URI 必须带 serial。`backend.adb.{server_host, server_port, timeout_secs}` 都在 YAML 中可调（host/port 默认 `127.0.0.1:5037`）
- Secret 环境变量占位文件：`.env.example`（值用 `changeme`），`.env` 入 `.gitignore`；新增 secret 时必须同步更新

### 工厂与注入
- `adapters::backend::factory::BackendFactory` trait + `DefaultBackendFactory` 按 [`Location`] 装配 [`Backend`]：Local 直接给 `LocalBackend`；SMB / MTP / ADB 走 cfg-gated 分支：
  - `--features smb-backend` 启用：`RealSmbClient`（`smb_real.rs`，包 pavao + libsmbclient C 库；Mutex 串行化 + `unsafe impl Send+Sync`）；未启用时返 `Unsupported "smb backend not enabled; rebuild with --features smb-backend"`
  - `--features mtp-backend` 启用：`RealMtpClient` 当前是 stub（`mtp_real.rs`），运行期仍返 `Unsupported`，错误消息引导 future PR 选定 crate（libmtp-rs / gphoto2-rs / 自接 rusb-PTP，无现成跨平台 + Android NDK 友好方案）；未启用时返 `Unsupported "mtp backend not enabled; rebuild with --features mtp-backend"`
  - `--features adb-backend` 启用：`RealAdbClient`（`adb_real.rs`，包 adb_client 3.2；`Mutex<ADBServerDevice>` 串行化）；未启用时返 `Unsupported "adb backend not enabled; rebuild with --features adb-backend"`
- 测试侧 `tidy_with(factory, command)` 接 `BackendFactory` 注入：集成测试通过 `FakeBackendFactory`（`HashMap<scheme, Arc<dyn Backend>>`）挂载 `FakeBackend` 验证跨 scheme 调度；`FakeBackend` / `FakeOp` 已 `#[doc(hidden)] pub use` 到 crate 根，integration test 可直接 import

### ADB 特殊实现
- adb sync 协议无原生 unlink / mkdir，`RealAdbClient` 通过 `shell_command("rm -f ...")` / `shell_command("mkdir -p ...")` 补齐，shell 参数走 `adb::shell_quote` 单引号转义防注入
- adb_client `stat/list/pull/push` 接 `&dyn AsRef<str>` trait object，传 `&path: &&str` 是正确二级借用

### 远端 backend 测试套路（SMB / MTP / ADB 通用）
- **手写 FakeSmbClient / FakeMtpClient / FakeAdbClient**：state 用 `Arc<Mutex<HashMap<...>>>`；`inject(*Op::Read, path, ErrorKind::TimedOut)` 注入逐 op + 逐 path 的错误，无须 `mockall`
- **错误文案映射**：
  - SMB `map_smb_error` 对 `io::Error::other` + 文案含 `"EACCES"` 转 `PermissionDenied`；FakeSmbClient 用同一文案触发
  - ADB `map_adb_error` 对 `io::Error::other` + 文案含 `"no such file"` / `"does not exist"` / `"device not found"` / `"no devices"` 转 `NotFound`，`"permission"` 转 `PermissionDenied`；FakeAdbClient 用特征文案触发
- **env 凭据传递**：`build_target` 读 `SMB_PASSWORD` / `KRB5CCNAME`；测试用 `unsafe { std::env::set_var(...) }` + nextest 进程隔离让其安全（与 `OnceLock` / `expand_env` 测试同套路，见 P0.4 与 CLAUDE.md「工具链注意」）
- **真实 client 适配器**：
  - `RealSmbClient::{stat, list, read, write, unlink, mkdir}`（`smb_real.rs`，包 pavao）已接入，整模块标 `#![cfg_attr(coverage_nightly, coverage(off))]`（需 share 服务器才能稳定触发，CI 不可覆盖）
  - `RealMtpClient::new()` 是 stub 占位（`mtp_real.rs`），同样 coverage(off)
  - `RealAdbClient::{stat, list, read, write, unlink, mkdir}`（`adb_real.rs`，包 adb_client 3.2）已接入，整模块 coverage(off)
  - 调度逻辑（`build_target` / `parent_target` / `map_*_error` / `*BufferedWriter` / `adapters::dispatch::tidy_with` 各分支）默认编译走 fake 注入 100% 覆盖。`adapters::backend::factory::build_smb_backend` / `build_mtp_backend` / `build_adb_backend` 在 feature 启用时也标 coverage(off)（构造 Real* 可能需服务器；feature off 时的 Unsupported Err 分支默认编译可覆盖）
- **Backend trait 方法的 rejection 测试**：每个方法测三类输入——OK / client Err 注入 / 非自家 scheme 返回 `InvalidInput`
- **"未启用 feature 返 Unsupported" 类集成测试**：`tidy_rejects_adb_uri_*` / `default_factory_adb_without_feature_*` 必须 `#[cfg(not(feature = "adb-backend"))]` gate，否则启用 feature 跑 nextest 会 fail（默认 factory 真去构造 Real* client，可能 Ok）；SMB 同类测试历史上未 gate，启用 `smb-backend` 跑会失败，属 baseline 缺陷
- **FakeBackend `inject_reader_error`**：让 `open_read` 成功但返回的 reader 在 `read` 时立即 Err，专门覆盖调用方在 stream hash / `sniff_mime` 等位置的 `?` Err 分支

## 配置与日志
- 运行时配置：`config.yaml`（项目根）+ `src/usecases/config.rs`，`config()` 返回 `&'static Config`（`OnceLock`）
- 切换配置：`TIDYMEDIA_CONFIG=/path/to.yaml`；语法 `${VAR:-default}` 由 `expand_env` 自实现（不引 dotenv）
- `FAST_READ_SIZE` 因 `[0; FAST_READ_SIZE]` 栈数组要求编译期常量，**不外置**（R1 合理例外）
- 结构化日志字段约定：`feature` / `operation` / `result`（CLI 工具无 request_id/user_id）
- `UtcOffset::from_whole_seconds` 范围 ±25:59:59，越界返回 `None`，用 `.unwrap_or(UtcOffset::UTC)` 兜底
- **R1 外置范围**：`copy.timezone_offset_hours` / `copy.unique_name_max_attempts` / `exif.valid_date_time_secs` + `backend.smb.{default_user,workgroup,timeout_secs}` / `backend.mtp.{device_match,storage_match}` / `backend.adb.{server_host,server_port,timeout_secs}` 需运维可调。其余 const 属 R1 边界例外**不外置**：
  - **spec §X 算法常量**：`EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS` / `MTIME_VS_P0_HINT_SECS`（filter/resolve）
  - **协议字面量**：`PHONE_PREFIX="IMG_"` / `CAMERA_PREFIX="DSC_"` / `SCREENSHOT_PREFIX="Screenshot_"` / `XMP_KEY` / `META_TYPE_IMAGE` / `META_TYPE_VIDEO`
  - **日志维度名**：`FEATURE_CLI` / `FEATURE_COPY` / `FEATURE_FIND` / `FEATURE_INDEX`
  - **lookup 表**：`MONTH: [&str; 13]`（copy.rs 月份零填充表）
  - **流式哈希**：`FAST_READ_SIZE`（栈数组要求编译期常量）/ `STREAM_CHUNK = 1 MiB`（远端 backend syscall 频率 vs 网络往返平衡）/ `MIME_SNIFF_BYTES = 256`（`infer::get` 仅看前 16-32 字节）
- `src/usecases/copy.rs` 的 `println!("\"{}\"\t\"{}\"", src, dst)` 是 CLI 脚本可读输出（dry-run + 完成回执），**不是** R3 日志路径，不要改成 tracing

## Android / 移动端（feature `android-app`）
- uniffi 0.31 proc-macro 模式：lib.rs 顶层 `uniffi::setup_scaffolding!()` 一次设置 + `#[uniffi::export]` / `#[derive(uniffi::Record)]` / `#[derive(uniffi::Error)]` 注解；不需要 build.rs / .udl 文件
- **uniffi 0.31 已知坑：`#[derive(uniffi::Error)]` enum 变体字段名不能叫 `message`**——uniffi 生成 `class Generic(val message: String)`，与 `kotlin.Exception.message` 撞名编译失败；用 `text` / `detail` 替代。同类 Throwable getter 名（`cause` 等）也要避开
- `[lib] crate-type = ["cdylib", "rlib"]`：cdylib 给 Android JVM dlopen，rlib 让桌面集成测试链得上；不要换 staticlib（uniffi 0.31 走 cdylib + JNA）
- 交叉编译：`cargo ndk -t aarch64-linux-android -p 30 --output-dir mobile/android/app/src/main/jniLibs build --release --features android-app`，`-p 30` 对齐 minSdk
- APK 复用顶层 `[profile.release]`：改 `opt-level` / `debug` / `lto` 会同步影响 jniLibs `libtidymedia.so` 的体积与运行性能；想分流要新建 `[profile.release-android]` 并改 `build-android.sh` 加 `--profile release-android`
- Kotlin 绑定：`uniffi-bindgen generate --library <libtidymedia.so> --language kotlin --out-dir <dir>`，输出 `uniffi/<crate>/<crate>.kt`；Kotlin 端通过 JNA 自动 dlopen jniLibs/arm64-v8a。**`uniffi --features cli` 装出的 binary 实际叫 `uniffi-bindgen`**（不是 `uniffi-bindgen-cli`，crates.io 没那个包）
- `src/mobile.rs` 无独立 use case：直接 `tidy_with(&DefaultBackendFactory, Commands::Copy {..})` 复用 CLI 路径（YAGNI）；feature off 时整模块 cfg 排除，default 覆盖率统计不参与
- mobile.rs `tidy_with(...)?` 的 Err 边在 DefaultBackendFactory + Local 路径几乎不可触发，feature on 时 stable region 1 miss 可接受（default 编译不参与统计，TOTAL 不受影响）
- **实测可工作工具组合（2025-09 后）**：JDK 25 (Temurin) + Gradle 9.1 + AGP 8.10 + Kotlin 2.0.21 + NDK r26d + SDK android-35。AGP 8.7 不支持 JDK 25；要升级 AGP 一起改
- **ANDROID_HOME ≠ ANDROID_NDK_HOME**：cargo-ndk 只读 NDK；Gradle build 还需 SDK（android.jar / aapt2 / build-tools），两个环境变量都要设
- **Compose 项目的 XML theme**：不要用 `@style/Theme.Material3.DayNight`（属 Material Components 库），Compose 项目自定义继承 `android:Theme.Material.Light.NoActionBar` 即可，颜色 / 排版完全交给 Kotlin 端 `MaterialTheme {}`
- **AGP 8.7+ / Kotlin 2.0+ 的 `android.kotlinOptions` 已 deprecated**：改用 `kotlin { compilerOptions { jvmTarget.set(JvmTarget.JVM_17) } }`，需 `import org.jetbrains.kotlin.gradle.dsl.JvmTarget`
- 静态校验 APK 不需要模拟器：`$ANDROID_HOME/build-tools/35.0.0/aapt2 dump packagename app.apk` / `aapt2 dump xmltree --file AndroidManifest.xml app.apk` / `unzip -l app.apk | grep lib/`

## 项目 Gotcha
- nextest 每个测试独立进程，`set_var`/`remove_var`/`OnceLock` 不会跨测试污染（区别于 `cargo test`）
- Cargo.toml 多数 dep 用 `"*"` 通配；`cargo update` 可能拉到不兼容主版本（已踩坑：sha2 0.10→0.11 把 `Digest::Output` 从 `GenericArray` 改成 `hybrid_array::Array`，导致 `SecureHash` 别名编译失败）
- `SecureHash` 别名走 `sha2::digest::Output<Sha512>`（即 `hybrid_array::Array<u8, U64>`），不是 `generic_array::GenericArray`；从 `Vec<u8>` 构造必须用 `SecureHash::try_from(vec.as_slice())`，直接 `try_from(vec)` 类型推断不过
- 仓库 baseline 已有 clippy errors（`io_other_error` 等），改动前先 `git stash` 跑 baseline 再对照
- HashMap 并行 in-place 改 value：用 `self.files.par_iter_mut().for_each(|(k, v)| ...)`，避免"par_iter→Vec→再 get_mut Option None"的不可达分支
- **测试 shim 必须 `#[cfg(test)]` gate**：`Info::from` / `Index::visit_dir` / `Exif::from_path_with_offset` 是包 backend-aware API 的旧入口（仅测试用，生产走 `*::open` / `visit_location`）；`adapters/backend/fake_remote` 整模块同理（`*_tests.rs` 专用，无生产消费）。未 gate 会让 release build 报 `dead_code`
- **`#[cfg(test)]` 标在方法/import 上，不要标在 `impl Foo {}` 块上**：同块生产方法会被一起 gate 掉。清 warning 时 `cargo build --release` 与 `cargo build --tests` 是不同 cfg，两边都要跑
