@~/.claude/rust-p0.md

@~/.claude/rust-p1.md

# tidymedia 开发上下文

按「拍摄时间」去重并整理照片/视频的多后端 CLI：扫描 sources（local/smb/adb/mtp 可混合）→ SHA-512 去重 → 按解析出的拍摄时间归档到 `output/年/月`。核心算法 = 拍摄时间判定（P0–P4 优先级），spec 见 `docs/media-time-detection.md`（代码内「spec §X」均指该文件）。Clean Architecture 四层 + Android app（feature `android-app`）。

## Quick Start
- 构建：`cargo build`；运行：`cargo run -- copy /source -o /output`；dry-run：`cargo run -- copy /source -o /output --dry-run`
- 测试：`cargo nextest run --release`；覆盖率：`cargo +nightly llvm-cov --release nextest --summary-only`
- lint：`cargo +nightly fmt && cargo clippy --all-targets --all-features --locked -- -D warnings`（默认与 `--all-features` 均 0 warning）

## 系统依赖与库特性
- 无外部进程依赖；EXIF/视频元数据走 `nom-exif`（图片+视频）+ `infer`（magic-bytes MIME）
- nom-exif 内部用 `tracing::info!/warn!` 大量输出，`install_logging` 用 EnvFilter 把 `nom_exif=error` 默认压住，保留 `RUST_LOG` 覆盖
- nom-exif 3.5 把 MKV `DateUTC` 合并到 `TrackInfoTag::CreateDate`（无独立 tag）；区分 MP4/MOV vs MKV/WebM 需 MIME 嗅探（`video/x-matroska` / `video/webm`）分流 `Source::MkvDateUtc` vs `QuickTimeCreateDate`
- nom-exif `Exif::get(tag)` 仅读 IFD0/MAIN；GPS 子 IFD 标签必须用 `Exif::iter()` 按 tag code 匹配

## 测试与覆盖率（项目特有；通用套路见 rust-p1 §5）
- 默认 stable：`cargo +nightly llvm-cov --release nextest --summary-only`（~99.6% region）
- 严格 100%：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only [--branch]`；`lib.rs` / `bin/tidymedia.rs` 顶部 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]`；`Cargo.toml [lints.rust] unexpected_cfgs` 注册 `cfg(coverage_nightly)`；不可稳定触发分支用函数级 `#[cfg_attr(coverage_nightly, coverage(off))]`
- **`--branch` multi-binary instance 陷阱**：lib unit + 集成 binary 各自 codegen 热点 fn 副本，每副本独立计数器；某 binary 未触发即报 instance miss。可行：①重构成 `?`（算 region 不算 branch）；②函数级 `coverage(off)`（已用于 `Info::open` / `calc_full_hash` / `secure_hash` / `create_time` / `PartialEq::eq` / `full_path` / `do_copy`）
- 改 `Cargo.toml` / `coverage` 属性后必跑 `cargo +nightly llvm-cov clean --workspace`
- `FakeBackend::inject_reader_error`：`open_read` 成功但 reader `read` 立即 Err，覆盖 stream hash / `sniff_mime` 等 `?` Err 分支

## Fixture
- `tests/data/` 下文件 mtime 每次 `git checkout` 重置；时间相关测试 **MUST** 用 `filetime::set_file_mtime` 固定（封装：`entities/test_common::copy_png_to`，固定到 `FIXED_MEDIA_MTIME` = 2024-01-01 12:00:00 UTC）
- MP4 不传 `-metadata creation_time=` 时 nom-exif 返 `Some(1904-01-01)`（QuickTime epoch），要 None 用 MKV
- `camino::Utf8Path` 在 Linux 上**不**把 `\` 当分隔符，Windows 反斜杠路径测试行为不同

## 文件组织
- 单文件 > 512 行拆测试：`#[cfg(test)] #[path = "X_tests.rs"] mod tests;`
- CA 重构移动文件：`pub use A::B` 只 re-export 类型不保留路径；`pub mod B { pub use A::B::*; }` 保留完整模块路径；配套测试 `super::` 需改 `crate::` 绝对路径
- CA 依赖方向验证：`rg "use crate::adapters|use crate::frameworks" src/entities/ src/usecases/` 应仅返回 re-export 桥接
- 集成测试拆分：`tests/<name>.rs` 是 root binary，`tests/<name>/*.rs` 子目录 **不**会被当独立 binary；root 用 `#[path = "<name>/sub.rs"] mod sub;` 装配（参考 `tests/lib_tidy.rs`）

## 同步检查点（改 X → MUST 同步 Y）
> 字面默认值变更先 `rg <旧值>` 兜底改全。

- **新增 `Location` variant / backend scheme** → `entities/uri.rs` 的 `FromStr` + `adapters/backend/factory.rs`（cfg-gated 分支 + Unsupported 兜底）+ 对应 `Backend` 实现 + `adapters/dispatch.rs` 调度 + 本文「URI 格式」节
- **新增 `Backend` trait 方法** → 全部 7 个实现同步加默认或 override：`local`/`remote`/`smb`/`adb`/`mtp`/`fake`/`fake_remote`；按「远端测试套路」补 OK / client Err 注入 / 非自家 scheme 三类测试
- **新增配置字段** → `usecases/config.rs` 结构体 + `config.yaml` + `validate_*` 校验或被消费（杜绝哑配置）；secret 再加 `.env.example`（值 `changeme`）+ 确认 `.env` 已 gitignore
- **新增 CLI flag** → `adapters/dispatch.rs` 调度透传 + **每个子命令路径（copy/move/find）独立 e2e 触发 Some/None 两边**，否则 LLVM branch miss
- **新增 `media_time` 候选来源 / 调整 P0–P4** → 先改 `docs/media-time-detection.md` spec → `priority.rs` 枚举 → 对应解析模块 → `resolve`/`decision` 裁决 → 补 fixture

## 项目分层（Clean Architecture）
- 四层（自外向内）：`src/frameworks/` → `src/adapters/` → `src/usecases/` → `src/entities/`
- `bin/tidymedia.rs` **只**调 `tidymedia::run_cli(env::args_os())`，零业务逻辑；`lib.rs` 仅模块声明 + re-export
- `usecases/` 仅依赖 `entities/`；通过 `pub use crate::frameworks::config::config;` re-export 让 usecases 内部用 `super::config::config` 不直接依赖 frameworks
- `entities/backend/` 是 Gateway 抽象（`trait Backend` + `SmbTarget` / `AdbTarget` / `MtpTarget`）；具体实现在 `adapters/backend/` 通过 re-export 模块保持原有路径；`file_info` / `file_index` / `exif` / `media_time::sidecar` 都 backend-aware（持 `Arc<dyn Backend>`）
- 目录名是 `usecases`（无下划线）

## 核心算法：media_time
- spec：`docs/media-time-detection.md`（§3 P0–P4 来源等级，§6 mtime 提示性冲突阈值）
- `entities/media_time/` 8 子模块单一职责：`priority`（P0–P4 枚举）/ `candidate`（`epoch_to_candidate`：secs==0 视为未填返 None）/ `filename`（P2 启发式：`IMG_`/`DSC_`/`Screenshot_` 前缀 + 13 位毫秒 Unix 戳）/ `filter`（合理性过滤 + `EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS`）/ `resolve` + `decision`（多候选裁决）/ `fs_time`（P3/P4）/ `sidecar`（XMP 旁车，backend-aware，sibling 路径计算当前仅 Local）
- **`Info::create_time` 不消费 P2 filename 候选**（spec §2.P2 与代码偏差）：`copy.rs::do_copy` 只看 EXIF + fs_fallback。`archive_template` 端到端测试**必须**选含 EXIF 的 fixture（如 `sample-with-offset.jpg`），不能用 P2 文件名 fixture

## URI 与 Backend

### URI 格式
- CLI `sources` / `output` 接 `Location`（实现 `FromStr` 让 clap 自动 value_parser）：
  - 无 `://` 或 `local://` ⇒ `Local`
  - `smb://[user@]host[:port]/share/path` ⇒ `Smb`
  - `mtp://device/storage/path` ⇒ `Mtp`
  - `adb://[serial]/abs/path` ⇒ `Adb`；serial 为空（`adb:///sdcard/...`）让 client autodetect；path 始终是设备绝对路径
- 字段内空格/中文/分隔符走 `percent-encoding`，**不引** `url` crate
- 混合 sources 已支持：`copy smb://a /local/b mtp://c adb:///sdcard/d -o /x` 合法

### 凭据
- SMB：`SMB_USER` 经 `backend.smb.default_user` 兜底；`SMB_PASSWORD` 在 `build_target` 处读 env；Kerberos 走 `KRB5CCNAME`；`backend.smb.workgroup` 默认 `WORKGROUP`。**密码永远不入 YAML**
- ADB：走本机 `adb` daemon 协议（adb_client 3.2 通过 TCP 连 `127.0.0.1:5037`）；运行前需 `adb start-server`、设备开 USB 调试 + 文件传输；多设备 URI 必须带 serial

### 工厂与注入
- `adapters::backend::factory::BackendFactory` trait + `DefaultBackendFactory` 按 `Location` 装配：Local 直给 `LocalBackend`；SMB/MTP/ADB 走 cfg-gated 分支（`smb-backend`/`mtp-backend`/`adb-backend`）；MTP 当前 stub 运行返 `Unsupported`
- feature off 返 `Unsupported "<scheme> backend not enabled; rebuild with --features <scheme>-backend"`
- 测试侧 `tidy_with(factory, command)` 接 `BackendFactory` 注入；集成测试用 `FakeBackendFactory` 挂 `FakeBackend`；`FakeBackend` / `FakeOp` 已 `#[doc(hidden)] pub use` 到 crate 根

### 远端 backend 测试套路（SMB / MTP / ADB 通用）
- **手写 Fake\<Smb|Mtp|Adb\>Client**：state 用 `Arc<Mutex<HashMap<...>>>`；`inject(*Op::Read, path, ErrorKind::TimedOut)` 注入逐 op + 逐 path 错误，无须 `mockall`
- 错误文案映射：SMB `map_smb_error` 文案含 `"EACCES"` → `PermissionDenied`；ADB `map_adb_error` 文案含 `"no such file"` / `"device not found"` / `"no devices"` → `NotFound`，`"permission"` → `PermissionDenied`
- 真实 client 适配器（`*_real.rs`）整模块标 `#![cfg_attr(coverage_nightly, coverage(off))]`（需真实服务/设备无法 CI 触发）；调度逻辑（`build_target` / `map_*_error` / `tidy_with` 各分支）走 fake 注入 100% 覆盖
- 每方法测三类：OK / client Err / 非自家 scheme 返 `InvalidInput`
- **"未启用 feature 返 Unsupported" 集成测试 MUST `#[cfg(not(feature = "<scheme>-backend"))]` gate**，否则启用 feature 跑 nextest 会 fail

## 配置与日志
- 运行时配置：`config.yaml`（项目根）+ `src/usecases/config.rs`，`config()` 返 `&'static Config`（`OnceLock`）
- 切换配置：`TIDYMEDIA_CONFIG=/path/to.yaml`；语法 `${VAR:-default}` 由 `expand_env` 自实现（不引 dotenv）
- 结构化日志字段约定：`feature` / `operation` / `result`（CLI 工具无 request_id/user_id）
- `UtcOffset::from_whole_seconds` 范围 ±25:59:59，越界返 `None`，用 `.unwrap_or(UtcOffset::UTC)` 兜底
- **R1 外置**：`copy.{timezone_offset_hours, unique_name_max_attempts, archive_template}` / `exif.valid_date_time_secs` / `backend.smb.{default_user,workgroup,timeout_secs}` / `backend.mtp.{device_match,storage_match}` / `backend.adb.{server_host,server_port,timeout_secs}`
- **不外置的合理例外**：spec §X 算法常量（`EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS` / `MTIME_VS_P0_HINT_SECS`）/ 协议字面量（`IMG_` / `DSC_` / `Screenshot_` / `XMP_KEY`）/ 日志维度名（`FEATURE_*`）/ lookup 表（`MONTH`）/ 流式哈希（`FAST_READ_SIZE`、`STREAM_CHUNK = 1 MiB`、`MIME_SNIFF_BYTES = 256`）
- `src/usecases/copy.rs` 的 `println!("\"{}\"\t\"{}\"", src, dst)` 是 CLI 脚本可读输出（dry-run + 完成回执），**不是** R3 日志路径，不要改成 tracing

## Android / 移动端（feature `android-app`）
- uniffi 0.31 proc-macro 模式：lib.rs 顶层 `uniffi::setup_scaffolding!()` + `#[uniffi::export]` / `#[derive(uniffi::Record)]` / `#[derive(uniffi::Error)]` 注解；不需要 build.rs / .udl
- **`#[derive(uniffi::Error)]` 字段名不能叫 `message`**：uniffi 生成 `class Generic(val message: String)` 与 `kotlin.Exception.message` 撞名编译失败；用 `text` / `detail`。同类 Throwable getter 名（`cause` 等）也要避开
- `[lib] crate-type = ["cdylib", "rlib"]`：cdylib 给 Android JVM dlopen，rlib 让桌面集成测试链得上；不要换 staticlib
- 交叉编译：`cargo ndk -t aarch64-linux-android -p 30 --output-dir mobile/android/app/src/main/jniLibs build --release --features android-app`
- Kotlin 绑定：`uniffi-bindgen generate --library <libtidymedia.so> --language kotlin --out-dir <dir>`。**`uniffi --features cli` 装出的 binary 实际叫 `uniffi-bindgen`**（不是 `uniffi-bindgen-cli`）
- `src/frameworks/mobile.rs` 无独立 use case：直接 `tidy_with(&DefaultBackendFactory, Commands::Copy {..})` 复用 CLI 路径（YAGNI）；feature off 时 cfg 排除
- **`tidy_with` 单一入口返 `CommandResult` enum**（`Copy(CopyReport)` / `Find(FindReport)`）：CLI 走 `tidy(..).map(|_|())` 丢弃，mobile/Android 直接 `match` 取 report；**MUST NOT** 新增 `*_report()` 专用包装函数复制 dispatch 逻辑
- **mobile FFI 嵌套集合 MUST `Vec<Record>`**（如 `Vec<MobileDuplicateGroup>`），禁 `paths.join(",")` CSV——路径含逗号会被 Kotlin `split(",")` 拆错段；uniffi 0.31 原生支持嵌套 Record sequence
- 实测工具链：JDK 25 (Temurin) + Gradle 9.1 + AGP 8.10 + Kotlin 2.0.21 + NDK r26d + SDK android-35（AGP 8.7 不支持 JDK 25）。`ANDROID_HOME ≠ ANDROID_NDK_HOME`：cargo-ndk 只读 NDK，Gradle build 还需 SDK，两个环境变量都要设

## 项目 Gotcha
- Cargo.toml 多数 dep 用 `"*"` 通配；`cargo update` 可能拉到不兼容主版本（sha2 0.10→0.11 已踩坑），主版本升级前先 dry-run
- **测试 shim 必须 `#[cfg(test)]` gate**：`Info::from` / `Index::visit_dir` / `Exif::from_path_with_offset` / `adapters/backend/fake_remote` 是包 backend-aware API 的旧入口，仅测试用；未 gate 会让 release build 报 `dead_code`
- **`#[cfg(test)]` 标在方法/import 上，不要标在 `impl Foo {}` 块上**：同块生产方法会被一起 gate 掉。`cargo build --release` 与 `cargo build --tests` 两边都要跑
- **`--all-features` clippy 与 `#[cfg(not(feature))]` test 联动**：启用全部 feature 后，gate 掉的 test fn 对应 imports 必须用同样 `#[cfg(not(all(feature = "smb-backend", feature = "mtp-backend", feature = "adb-backend")))]` 包裹，否则 `unused_import` error
- **clippy 1.95 `doc_markdown` 扩大**：含点号的文件名、含下划线的标识符在 `///` 或 `//!` 注释中均需反引号包，否则 `--all-features` 下 `-D warnings` 报 error
- **`chrono::TimeDelta::seconds(i64)` 会 panic**（secs > ≈ `i64::MAX/1000`），`?` / `.ok()?` 截不住——外部 timestamp（Takeout JSON / 用户输入）解析 MUST 用 `try_seconds()?` + `DateTime::checked_add_signed`
- **重复组容器 MUST `Vec<DuplicateGroup { size, paths }>`**，MUST NOT `BTreeMap<size, _>`：size 作唯一键会让同 size 不同 content 的两组互相覆盖（`file_index.rs::filter_and_sort`）
- **路径前缀匹配 MUST 校验分隔符边界**：`"/photos_backup/x".starts_with("/photos")` 会误判为 output 内须保留——见 `find.rs::under_prefix` 已封装
- **`ReportSink` trait 用 `enum Report<'a>` + 单方法 `write(&Report<'_>)`** 收敛多报告类型；对象安全 + 新增 report 变体不强制升级既有 impl（替代旧 `write_copy` / `write_find` 双方法 boilerplate）
