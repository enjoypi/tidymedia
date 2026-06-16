@~/.claude/rust-p0.md

@~/.claude/rust-p1.md

# tidymedia 开发上下文

按「拍摄时间」去重整理照片/视频的多后端 CLI：扫描 sources（local/smb/adb/mtp 可混合）→ SHA-512 去重 → 按拍摄时间归档到 `output/年/月`。核心算法 = 拍摄时间判定（P0–P4，见「核心算法」节）。Clean Architecture 四层 + Android app（feature `android-app`）。代码注释里的 `docs/media-time-detection.md §X` 是历史 spec 引用（文件已删）。

## Quick Start
- **所有 cargo 命令 MUST 带 `--release`**：`profile.release` 已设 `opt-level = 0`，编译速度与 debug 相当，统一 target 目录避免双份产物
- 构建/运行/dry-run：`cargo {build,run} --release [-- copy /src -o /out [--dry-run]]`
- 测试 `cargo nextest run --release`；覆盖率 `cargo llvm-cov --release nextest --summary-only`（严格 100% 口径见「测试与覆盖率」）
- lint `cargo +nightly fmt && cargo clippy --release --all-targets --all-features --locked -- -D warnings`；**`--all-features` 仅 Linux 可验**（smb-backend 需 libsmbclient）
- **CLI flag 位置**：`--log-level=debug` 是全局 flag 放最前；`--dry-run` 是子命令级 MUST 放 `copy`/`move` 后
- tidymedia debug 走 stderr；`| tail -N` 会截 copy_file 行让 summary `copied=N` 与日志行数对不上 → 重定向文件再 grep
- MSVC 链接固化在 `.cargo/config.toml`（PATH 中 GNU coreutils `link` 会抢 `link.exe`、vcvars64 在精简 shell 不可用）；换 MSVC/SDK 版本需同步该文件
- rustup default-host + 全部工具链 MUST `x86_64-pc-windows-msvc`（gnu 与 MSVC 链接不兼容）；cargo-llvm-cov 安装不带 `--locked`
- git commit 格式：`type: 中文描述`（本项目无 TASK-ID），按主题拆分

## 系统依赖与库特性
- 无外部进程依赖；EXIF/视频元数据走 `nom-exif`（图+视频）+ `infer`（magic-bytes MIME）
- `bin/exiftool/exiftool.exe`（仅开发调试）：常用 `-time:all -s <f>` 查时间字段、`-P '-AllDates<Filename' -overwrite_original <dir>` 按文件名批量改时间保 mtime；单文件按值写 `-P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=..."`
  - **中文路径**：portable exiftool 缺 `Win32API::File`，命令行 UTF-8 直传 0 文件；要 UTF-8 路径走 **tempfile + `-@ <file>` + `-charset filename=UTF8`**（小写 `filename`）；省掉 `-charset` 走 ANSI 系统调用也稳（stdout 乱码不影响 EXIF 抽取）
  - 写操作默认产 `<file>_original` 备份，MUST 清理（magic-bytes 仍 JPEG 会被 tidymedia 当媒体归档）
  - **`-T` Windows 末字段带 `\r`**：CRLF 让 awk `$NF` 拼成 `value\r`，终端 `\r` 盖行首致 print 错乱、`$NF=="value"` 永假；awk 入口 MUST `sub(/\r$/, "", $NF)`。**项目内已统一迁 Python**（见末节「dry-run log / 路径聚合用 Python」），此坑仅在临时手写 awk 调试时遇到
- nom-exif 内部 `tracing::info!/warn!` 大量输出，`install_logging` 用 EnvFilter 默认压 `nom_exif=error`，保留 `RUST_LOG` 覆盖；trace 调试 `RUST_LOG=nom_exif=trace`，span 名是 `#[tracing::instrument]` 函数名嵌套
- nom-exif 3.5 把 MKV `DateUTC` 合并到 `TrackInfoTag::CreateDate`（无独立 tag）；MP4/MOV vs MKV/WebM 走 MIME 嗅探（`video/x-matroska` / `video/webm`）分流 `Source::MkvDateUtc` vs `QuickTimeCreateDate`
- nom-exif `Exif::get(tag)` 仅读 IFD0/MAIN；GPS 子 IFD 标签必须 `Exif::iter()` 按 tag code 匹配
- **老 QuickTime `pnot`/`mdat` 首 box MOV**（NIKON COOLPIX S5/P5000、Casio 等 2003-2008 早期机型，及 `moov` 在文件末尾的 mdat-first 变体）nom-exif 3.6.0 `parse_bmff_mime` 拒识。fork patch 在入口短路返 QuickTime mime（**必须 `BoxHeader::parse` 仅解析 header**——`BoxHolder::parse` 要求整 body 入 buffer，数 MB mdat 必 Incomplete 让短路静默失效）：`Cargo.toml [patch.crates-io] nom-exif = { git = "https://github.com/enjoypi/nom-exif.git", branch = "feat/legacy-quicktime-pnot" }`；上游 PR 合并后切回 `*` 删 patch
- **AVI（RIFF）nom-exif 不支持**：老相机（Fujifilm FinePix 等）拍摄时间在 `LIST hdrl > LIST strl > strd` chunk（`AVIF` 魔数 + 裸 TIFF IFD，offset 基准 = strd+8），`entities/riff.rs` 自解析填 `Exif` 的 DTO/CreateDate/ModifyDate/make/model；`entities/exif/types.rs::from_reader` 按 `video/x-msvideo` 先于泛 video 分流。**IFD 字节解析抽 `entities/tiff_ifd.rs` 公共模块**（支持 II/MM byte order + IFD0 + ExifIFD 子 IFD），与 PNG eXIf chunk / JPEG APP1 fallback 共享，DRY
- **PNG `eXIf` chunk nom-exif 不解析**：PNG 1.5+ 自定义 chunk `eXIf` = 完整 TIFF/EXIF header（II/MM + 0x002A magic + IFD0 + ExifIFD），Lightroom/相机直出 PNG 常含。`entities/png.rs` 自解析（chunk header BE u32 长度 + 4 字节 type，不验证 CRC YAGNI），`entities/exif/image_png.rs::populate_png_dates` 调 `tiff_ifd::parse_tiff` 抽 DTO/CreateDate/ModifyDate/Make/Model；`types.rs::from_reader` 按 `image/png` 先于泛 `image/` 分流。eXIf 缺失或 TIFF 损坏退回 XMP fallback（Lightroom 常并行写 PNG eXIf + XMP）；fixture `tests/data/sample-png-exif.png`（`tests/fixtures/gen_png_exif.py` 生成）
- **JPEG nom-exif `parse_exif` 整体失败 fallback**：Canon EOS 7D 等 MakerNotes 偏移异常机型（exiftool 报 `Adjusted MakerNotes base by -126`）让 nom-exif 抛 Err 致 DTO/CreateDate/Make/Model 全丢。`entities/exif/image_jpeg.rs::parse_jpeg_app1_exif` 扫 JPEG APP1 段（`FF E1` marker + BE u16 长度 + `Exif\0\0` magic + 完整 TIFF header）调 `tiff_ifd::parse_tiff` 抽核心字段；`populate_image_dates` 的 nom-exif Err 分支前置 fallback，三态闭合（fallback 成功 → 直填 / fallback Err → 退 XMP fallback / 主路径 Ok 但双 0 → XMP fallback）。fixture `tests/data/sample-jpeg-app1-broken.jpg`（合成，模拟 ExifIFD 越界 count；累积真实 7D 样本后替换）
- **M2TS/AVCHD nom-exif 不支持**：Canon 摄像机时间在 H.264 SEI `user_data_unregistered`（MDPM UUID），`entities/m2ts.rs` 按 AVCHD video PID `0x1011` 重组 192B BDAV payload 直搜 UUID（不解析 PMT/NAL，YAGNI），EP 字节 `00 00 03` 按 RBSP 剥离。tag `0x18`=[时区,世纪,年,月] BCD、`0x19`=[日,时,分,秒] BCD；时区字节忽略走 naive+配置时区；仅填 DTO（单一拍摄时刻不伪造 P1）；fixture `tests/data/sample-canon-avchd.m2ts`
- **JPEG/HEIC/TIFF XMP fallback**：Lightroom/Bridge/ACR re-tag 后 EXIF IFD0 可能仅剩 `ModifyDate`，原始时间在 XMP `photoshop:DateCreated`（→ DTO/P0）与 `xmp:CreateDate`（→ CreateDate/P1）。`populate_image_dates` 双 0 时扫已 buffer 头 64 KB（`XMP_SCAN_BYTES`），`entities/xmp.rs` 抽 attribute 形式（element/ExtendedXMP YAGNI），`adapters/sidecar.rs::parse_xmp_date` 复用同实现；fixture 用 `-api Compact="Shorthand,NoPadding"` 才出 attribute 形式
  - **seek 失败仍跑 XMP fallback**：head 已读入不浪费，否则非可寻址 reader 的 re-tag 图退回 mtime 归错桶
  - **XMP 未闭合 `<!--` 按 strict 抹至 EOF**：lenient 会把注释中 `photoshop:DateCreated="OLD"` 误读为合法属性

## 测试与覆盖率（项目特有；通用套路见 rust-p1 §5）
- **覆盖率门槛 region/function/line/branch 四项 MUST 全 100%**（Linux + `--all-features` + ignore-regex 严格口径 + `--branch`）。**Windows 默认 feature 口径可能有平台差异 miss**（`uri.rs`/`sidecar.rs`/`filename.rs`/`resolve.rs`，多为 feature-gated + multi-binary instance）：Windows 本机判定 = 新增代码 100% + TOTAL miss 与 main 持平；怀疑回归先 `git stash` 对照
- 严格 100%：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch --ignore-filename-regex='_real\.rs$'`；`lib.rs` / `bin/tidymedia.rs` 顶 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]`；`Cargo.toml [lints.rust] unexpected_cfgs` 注册 `cfg(coverage_nightly)`
- **`*_real.rs` 走 `--ignore-filename-regex` 而非 `coverage(off)`**：真实 client 适配器调真实 SMB/adb-server CI 不可触发，但 `coverage(off)` 注解侵入源码且 mutation testing 会扫到；命令行 `--ignore-filename-regex='_real\.rs$'` 排除整文件，源码零 attribute。`factory.rs` 的 cfg-gated 真实装配（`build_smb_backend` 等）抽到 `adapters/backend/factory_real.rs` 符合命名约定，由同 regex 排除；Unsupported feature-off 分支保留在 `factory.rs` 由 `tidy_rejects_*` e2e 100% 覆盖
- **`--branch` multi-binary instance 陷阱**：lib unit + 集成 binary（含 `tests/lib_tidy.rs` + `tidymedia` 二进制 subprocess）各 codegen 热点 fn 副本，每副本独立 BR/region 计数；cargo-llvm-cov 累加 instance 间 hit，任一 instance hit ≥1 即 covered。解法：①重构成 `?`（算 region 不算 branch）；②显式 `match` 替 `.map_or_else(closure, closure)`（closure 在每 instance 独立编译，未触发即 0-hit，`match` arm 让 LLVM 跨 instance 合并 region）；③`#[doc(hidden)] pub use <internal_helper> as __<name>;` 把私有 fn 在 lib.rs 暴露给 `tests/lib_tidy/*.rs` 直接调用，触发 lib_tidy binary instance 的 0-hit region/分支（参考 `__compute_output_prefix` / `__parse_xmp_dates`）
- 改 `Cargo.toml` / `coverage` 属性后必 `cargo +nightly llvm-cov clean --workspace`
- `FakeBackend::inject_reader_error`：`open_read` 成功但 reader `read` 立即 Err，覆盖 stream hash / `sniff_mime` 等 `?` Err 分支；`add_file_with_times` 构造 `modified=None`（真实 fs 造不出，`create_time` 边界用）；`DummyClient.mkdir_calls` 原子计数杀「best-effort 调用替换成 no-op」类变异
- **fast-path 命中反向证据**：`fs::rename`/`fs::copy` 保留 src mtime，`std::io::copy` 不保留；`filetime::set_file_mtime` 钉 src 后 move 断言 dst mtime 不变即命中（参考 `tests/lib_tidy/move_idempotency.rs::move_local_fastpath_dst_mtime_matches_src`）。fast-path 走 `fs::rename` 不读源，chmod 触发失败改 chmod 555 **src 父目录**（rename 需父目录 write）
- **chmod 触发 stat 类 IO 错误**：chmod 0 **文件** stat 仍成功（要 parent dir execute）；chmod 0 **parent dir** 才让 `try_exists`/`metadata` 返 `PermissionDenied`
- **测试 env helper 套路**（P0 §13 SAFETY DRY）：测试模块顶 `#![allow(unsafe_code)]` + 单点 `fn set_env_var(name, value) { unsafe { std::env::set_var(name, value) } }` / `remove_env_var`（参考 `frameworks/config.rs::tests`）。**sed 批量替换陷阱**：把 helper 自身实现也匹配掉致无限递归 → sed 后手动改回 helper 内 unsafe 块
- **平台条件常量优先于 `if cfg!(...)`**：跨平台行尾/路径分隔符 MUST `#[cfg(target_os="windows")] pub(crate) const X: &str = "\r"; #[cfg(not(...))] pub(crate) const X: &str = "";` 而非 `if cfg!(target_os="windows")`。`cfg!()` 让 LLVM instrument 两 arm、dead arm 必 miss（实测 region +11/line +3 miss）；双 `#[cfg]` 让 cfg-out 平台代码不编译。**业务逻辑跨平台分裂应消除而非容忍**——find 删除脚本统一 Python 已消除 3 处 cfg
- **Windows 同卷边界本地测**（`tests/lib_tidy/windows_same_volume.rs`，`#![cfg(windows)]`）：`subst` + `mklink /J` 构造「不同前缀同卷」均不要 admin；subst 盘字全局共享 → `.config/nextest.toml` 加 `[test-groups.windows-volume-mut] max-threads=1` + filter 强制串行；释放盘字用 RAII Drop guard `subst Y: /D`
- **`--all-features` 覆盖**：`dispatch.rs` 的 `usecases::copy(..)?` Err arm，现有 `tidy_rejects_*` 用 `#[cfg(not(feature="smb-backend"))]` gate，全 feature 跑覆盖率不编译 → 用 output 父路径占成普通文件触发 `mkdir_p` Err 兜底（参考 `dispatch_and_cli.rs::tidy_copy_propagates_mkdir_error_when_output_parent_is_file`）
- **`--all-features` 严格覆盖也须 100%**：feature 启用侧用 `#[cfg(feature="...")]` 镜像 `tidy_rejects_*`（`tests/lib_tidy/real_factory.rs`）。`RealMtpClient::new` 是 stub 必 Err → 确定性触发 `dispatch.rs` 各 `?` Err arm；`PavaoClient::new`/`ADBServerDevice::new` 仅初始化不连网可直接断言。feature 启用时 `factory.rs` 用 `use super::factory_real::build_smb_backend;` 形式替换 dispatch 目标，dead 兜底走 ignore-regex 排除（不用 `coverage(off)`）。`mobile.rs` 不可达防御抽纯 helper（`expect_copy`/`expect_find`/`copy_status`/`stats_from`/`mobile_report_from`）直测；调用点用 `.map` 替代 `?` 消除永不触发的 Err region
- **子行 region miss 定位**（lcov `DA`/`BRDA` 不显示）：`cargo +nightly llvm-cov report --release --text` 复用上次 profdata（**`report` 子命令不接 `--all-features`**，feature 集随上次 run），输出 `^0` 标记即 miss
- **branch miss 精确定位**：`llvm-cov report --release --lcov` 过滤 `BRDA` 第 4 字段 0
- **逻辑不可达的 `?` 死区消除**：循环边界用 `while let Some(x) = buf.get(start..end)` 替手写哨兵（参考 `entities/riff.rs`）；`buf.get(idx..)?` 当 idx 来自 `position()`/`find()` 时改 `&buf[idx..]`（`?` Err arm 不可达，参考 `entities/xmp.rs::find_xmp_packet` / `find_attr_rfc3339`）；纯类型安抚用 `unwrap_or(常量)` 替 `.ok()?`
- **fn pointer 依赖注入测 Err arm**：thin wrapper 在生产路径下 OK arm 必触发但 Err arm 难真触发（如 `walk_entry_to_io` 的 `entry.metadata()` 失败、`rename_or_fallback` 的 `remove` 失败）→ 抽 `*_with(fn pointer)` 参数化版本，生产入口默认传 real fn，测试注 mock 返 Err 命中 `.map_err` 闭包 region（参考 `local.rs::walk_entry_to_io_with` / `rename_or_fallback_with`）
- **`DummyCtx::fail_after(n: usize)` 模式**：`from_location` 第 N 次调用之后开始返 Err，让 `walk_recursive` 的子项 `from_location` Err arm 在根 `build_target` 成功的前提下被命中（用 `Arc<AtomicUsize>` 计数；参考 `remote_test_helpers.rs::DummyCtx`）
- **assert! `&&` 拆两条 assert**：`assert!(a && b, ...)` 让 LLVM 把 `&&` 短路为两 sub-branch BR，False 路径（assert 失败）天然 0-hit；拆 `assert!(a); assert!(b);` 单 contains 不分子分支，BR 100%
- **`Cursor` seek 永不失败**：测 seek Err 传播需手写 FailSeek reader（包 Cursor、seek 恒 Err，blanket impl 自动满足 `MediaReader`）
- **mutation testing**：配置在 `.cargo/mutants.toml`（nextest + release + `--all-features`，排除 `*_real.rs`/测试基建；豁免在 `exclude_re` 注明 WHY）。**Windows 本机跑不了 `--all-features`**（smb 仅 Linux）：增量验证临时注释 `additional_cargo_args` 按默认 features 跑，feature-gated mutants 留 Linux。日常 `cargo mutants --in-diff <(git diff main)`；全量审计 ~677 mutants/~3h（2026-06 基线 509 caught / 168 unviable / 0 missed）；定点 `cargo mutants -F '<regex>'`（`-E` 是 exclude）。套路：①计数断言精确 `==`（`>= 1` 杀不掉 `+=`→`-=` usize release wrap MAX）；②与被调函数重复的入参 guard 等价变异，删冗余 guard 单点校验；③`cfg(windows)`/`cfg(not(feature))` 代码单口径必假幸存 → exclude_re；④tracing 字段值抽纯 fn 直测不靠捕日志；⑤防御性不可达 arm 喂错配变体直测；⑥写 kill 测试前确认变异行可达（如 `create_time` 无 EXIF let-else 早返回到不了被变异行）；⑦纯移动重构的 `--in-diff` 是假增量，可缩小到真改逻辑文件；⑧无返回值 tracing 副作用 fn `replace X with ()` 等价幸存 → exclude_re

## Fixture
- `tests/data/` 下文件 mtime 每次 `git checkout` 重置；时间相关测试 MUST 用 `filetime::set_file_mtime` 固定（封装 `entities/test_common::copy_png_to` 固定到 `FIXED_MEDIA_MTIME` = 2024-01-01 12:00:00 UTC）
- MP4 不传 `-metadata creation_time=` 时 nom-exif 返 `Some(1904-01-01)`（QuickTime epoch），要 None 用 MKV
- `FakeBackend` 默认 mtime = `UNIX_EPOCH` → `create_time` P4 兜底 + 默认时区 +8 → copy/move e2e 归档桶固定为 `1970/01`
- `camino::Utf8Path` 在 Linux 上**不**把 `\` 当分隔符，Windows 反斜杠路径测试行为不同

## 文件组织
- 文件 ≥400 行预防性拆至 ≤300。测试外置：`#[cfg(test)] #[path = "X_yyy_tests.rs"] mod yyy_tests;`；生产目录化 `foo/mod.rs`（仅声明+re-export）+ 子模块（参考 `entities/file_info/`、`usecases/copy/`）。测试要访问的内部项在 mod.rs `#[cfg(test)] use self::sub::item;` 私有 re-export（私有 use 对子模块可见）；跨子模块用 `pub(super)`，对外 API 用 pub-in-private-mod + `pub(crate) use`。子模块 MUST NOT 命名 `core`（与内建 crate 撞名）
- **大文件主题切多份 tests 外置**：tests 共享 helper 抽 `<mod>_tests_common.rs`（`pub(super) fn helper`），其他主题 `use super::tests_common::helper`；主 mod 顶层挂 `#[cfg(test)] #[path]` mod（**勿嵌套到 `mod tests {}`**——`#[path]` 在嵌套 mod 下基于父 mod 目录解析）。子 mod 跨引用其它 entities 项用 `crate::entities::...` 绝对路径而非 `super::super::...`（参考 `entities/exif/mod.rs` 6 主题 + tests_common、`entities/media_time/resolve_*tests.rs`、`frameworks/config_*tests.rs`）
- CA 重构移动文件：`pub use A::B` 只 re-export 类型不保留路径；`pub mod B { pub use A::B::*; }` 保留完整模块路径；配套测试 `super::` 需改 `crate::` 绝对路径
- CA 依赖方向验证：`rg "use crate::adapters|use crate::frameworks" src/entities/ src/usecases/` 应仅返回 re-export 桥接
- 集成测试拆分：`tests/<name>.rs` 是 root binary，`tests/<name>/*.rs` 子目录**不**会被当独立 binary；root 用 `#[path = "<name>/sub.rs"] mod sub;` 装配（参考 `tests/lib_tidy.rs`）

## 同步检查点（改 X → MUST 同步 Y）
> 字面默认值变更先 `rg <旧值>` 兜底改全。

- **新增 `Location` variant / backend scheme** → `entities/uri.rs::FromStr` + `adapters/backend/factory.rs`（cfg-gated + Unsupported 兜底）+ 对应 `Backend` 实现 + `adapters/dispatch.rs` 调度 + 本文「URI 格式」
- **新增 `Backend` trait 方法** → 全部 7 个实现同步加默认或 override：`local`/`remote`/`smb`/`adb`/`mtp`/`fake`/`fake_remote`；按「远端测试套路」补 OK / client Err 注入 / 非自家 scheme 三类测试
- **新增 `Backend` impl** → `walk` MUST 递归 yield 所有 file：LocalBackend 走 `WalkBuilder`、RemoteBackend 用 `walk_recursive` 自驱 `EntryKind::Dir` 子树；单层 list 会让 `Index::visit_location` 在 `kind != File` 过滤 Dir 永不下钻，远端子目录静默丢失
- **新增配置字段** → `usecases/config.rs` 结构体 + `config.yaml` + `validate_*` 校验或被消费（杜绝哑配置）；secret 加 `.env.example`（值 `changeme`）+ 确认 `.env` 已 gitignore
- **新增 CLI flag** → `adapters/dispatch.rs` 调度透传 + **每子命令路径（copy/move/find）独立 e2e 触发 Some/None 两边**，否则 LLVM branch miss；e2e MUST 含 `run_cli(["tidymedia", ...])` 字符串形式（clap flag 名映射只有该形式能验证）
- **新增 `media_time` 候选 / 调整 P0–P4** → `entities/media_time/priority.rs::Source`/`Priority` → 对应解析模块 → `resolve`/`decision` 裁决 → 补 fixture
- **新增 archive_template 占位符** → `usecases/archive_template.rs::render` 替换分支 + 同文件 `PLACEHOLDERS` 常量（当前 6 项）+ `usecases/config.rs::validate_archive_template`（在 `frameworks/config.rs::sanitize` 被调用）三处同步
- **新增容器/格式 EXIF 自解析** → `entities/<container>.rs`（chunk/segment 遍历）+ 调 `entities/tiff_ifd::parse_tiff`（PNG/JPEG header 入口）或 `parse_ifds`（裸 IFD 入口如 AVI strd）+ `entities/exif/image_<container>.rs` 或 fallback 接入点 + `entities/exif/types.rs::from_reader` 分流（image MIME 前置 / 视频 MIME 类似 `video/x-msvideo` 模式）+ 合成 fixture 与 `tests/fixtures/gen_<container>.py` Python 生成脚本入 `tests/fixtures/gen.sh`

## 项目分层（Clean Architecture）
- 四层（自外向内）：`src/frameworks/` → `src/adapters/` → `src/usecases/` → `src/entities/`
- `bin/tidymedia.rs` **只**调 `tidymedia::run_cli(env::args_os())`，零业务；`lib.rs` 仅模块声明 + re-export
- `usecases/` 仅依赖 `entities/`；通过 `pub use crate::frameworks::config::config;` re-export 让 usecases 内部用 `super::config::config`
- `entities/backend/` 是 Gateway 抽象（`trait Backend` + `SmbTarget`/`AdbTarget`/`MtpTarget`）；具体实现在 `adapters/backend/` 通过 re-export 保留原路径；`file_info`/`file_index`/`exif`/`media_time::sidecar` 都 backend-aware（持 `Arc<dyn Backend>`）
- 目录名是 `usecases`（无下划线）

## 核心算法：media_time
- 优先级：**P0** = `ExifDateTimeOriginal` / `QuickTimeCreationDate` / `MkvDateUtc`；**P1** = `ExifCreateDate` / `QuickTimeCreateDate`；**P2** = 文件名启发式；**P3** = `XmpSidecar` / `GoogleTakeoutJson`；**P4** = `FsMtime`
- mtime 比 P0 早 > 30 天发提示性冲突告警
- **多数派仲裁**：filename 与 mtime 互证（差≤1天）且与 P0 差>30天 → 推翻 P0（相机时钟错场景，`ConflictKind::P0OverruledByMajority` 不静默）
- **ModifyDate 三方互证否决**：互证成立但 filename 票与 EXIF `ModifyDate` 差≤1天 → 判 filename+mtime 为 re-save 时戳，保 P0 记 `MajorityVetoedByModifyDate`（`ModifyDate` 进 `Exif.modify_date` 但不进时间候选，仅作仲裁旁证）
- `entities/media_time/` 7 子模块单一职责：`priority`（`Source`+`Priority`）/ `candidate`（`epoch_to_candidate`：secs==0 视未填返 None）/ `filename`（P2：`IMG_`/`DSC_`/`Screenshot_` 前缀 + 13 位毫秒戳 + WeChat/WhatsApp/Pixel/裸 15 字符 `YYYYMMDD_HHMMSS` + 通用 `<任意前缀>YYYY-MM-DD HH-MM-SS` 19 字节窗口 + **宽松 YYYYMMDD**（stem 开头或 `-`/`_`/空格 后 8 位合法日期）） / `filter`（`EPOCH_1904`/`SOFT_THRESHOLD_1995`/`FUTURE_TOLERANCE_SECS`）/ `resolve`+`decision`（多候选裁决）/ `fs_time`（P4）
- P3 sidecar 在 `adapters/sidecar.rs`（Interface Adapter，不在 entities）：XMP `photoshop:DateCreated` 纯文本搜索 + Takeout `<media>.<ext>.json` `photoTakenTime.timestamp` 走 `serde_json`；backend-aware，sibling 路径计算当前仅 Local
- **P0–P4 全链接线 `Info::create_time`**：P0/P1 来自 EXIF；P2 由 `candidates_from_filename` 直接消费（文件名 naive 按 `timezone_offset_hours` 解释，与 EXIF naive 同口径——曾按 UTC 解释致月末晚间文件 +8 跨月归错桶）；P3 经依赖倒置注入——`dispatch` 把 `adapters::sidecar::discover_with_backend` 传给 `usecases::copy_with_sidecar`，`Index::enrich_candidates` 并行调 provider 填 `Info::add_candidates`；P4 = mtime。**e2e fixture 注意**：含 P2 文件名模式或带 `.json`/`.xmp` sidecar 的文件按文件名/sidecar 时间归档；`copy()`（无 provider）是 `#[cfg(test)]` shim，生产只走 `copy_with_sidecar`
- **EXIF vs 归档桶对账**：EXIF naive 按 `timezone_offset_hours`（默认 +8）转 epoch、归档再 `.to_offset(+8)` → 首尾抵消，**归档桶 = EXIF 字符串前 7 字符 `YYYY:MM`**，对账无需算时区。slash command `/tidy-verify <source> <output>`（`.claude/commands/tidy-verify.md` + `.claude/scripts/tidy-verify/`）：dry-run → exiftool 抽 → 桶 MISMATCH → 文件名时间 DIFFER → exiftool 修 → 真跑 move
- **QuickTime/视频容器时间是 UTC，对账须 +tz——「前 7 字符」规则仅对 EXIF naive 成立**：nom-exif 走 UTC epoch 再 `.to_offset(+tz)`，与 EXIF 首尾抵消口径不同；exiftool 默认 raw UTC 显示 QT 时间且**不加时区后缀**，`compare_buckets.py` 对 QT 列（`QT_COL_INDICES=(2,3)`）必须 `_qt_bucket()` 按 UTC→配置时区转换后取年月（带 `±HH:MM`/`Z` 后缀按其 offset 归一化）；否则月末 video 被误判 `TidymediaContainerMiss`。`03_compare_buckets.sh` 从 env > `config.yaml` 的 `timezone_offset_hours` > 8 解析时区
- **tidy-verify dry-run 年月分析**：直接按 `source=`/`target=` 父目录聚合会把根不同误判全变；MUST 剥源/目标根前缀到相对路径再比年月段
- **相机出厂默认时间陷阱**：EXIF P0 格式合法但值为出厂默认（如 2004-01-01 00:00），且早于机型发布日（FinePix E550 = 2004-07、Casio EX-Z50 = 2004-09）即判时钟未设；mtime 通常同错，多数派救不了，只能 exiftool 数据侧修复后再归档
- **多数派仲裁仅认 `Validity::Valid` 候选**：`apply_filters` 保留 `LowConfidencePre1995` 进 surviving；`majority_override` MUST 显式 `matches!(v, Validity::Valid)` 过滤低置信票，否则 1995 前 filename+mtime 互证可推翻可信 P0
- **冲突 `diff_secs` 约定** = `chosen.utc - other_utc`（`detect_conflicts` 各分支统一）；Override 分支 chosen=winner、other=原 P0 best；写反让 tidy-verify 判相机时钟方向颠倒
- **`EPOCH_1904` 过滤**用 `[EPOCH_1904, EPOCH_1904+86400)` 整天窗口——少数编码器把 `mvhd.creation_time` 写 1/2/... 让 ts=EPOCH_1904+1 漏过精确匹配
- **`Screenshot_` 截图两主流命名**：`yyyy-mm-dd-HH-mm-ss`（19 字符，Windows Snip & Sketch）与 `yyyymmdd_HHMMSS`（15 字符，Samsung/MIUI/原生 Android），`try_screenshot` 都要支持，否则后者退到 `try_loose_yyyymmdd` 兜底退化为日精度丢时分秒
- **copy/move 重叠保护**（`copy/run.rs`）：source ⊆ output → InvalidInput 拒绝（move 会把源判副本删除）；output ⊂ source（就地归档）→ `Index::remove_under_prefix` 把已归档剔除。前缀比较用 `entities::common::under_prefix`（分隔符边界），Local 走 `full_path`（绝对透传、相对才 canonicalize）、远端用 display

## URI 与 Backend

### URI 格式
- CLI `sources` / `output` 接 `Location`（实现 `FromStr` 让 clap 自动 value_parser）：
  - 无 `://` 或 `local://` ⇒ `Local`
  - `smb://[user@]host[:port]/share/path` ⇒ `Smb`
  - `mtp://device/storage/path` ⇒ `Mtp`
  - `adb://[serial]/abs/path` ⇒ `Adb`；serial 为空让 client autodetect；path 始终是设备绝对路径
- 字段内空格/中文/分隔符走 `percent-encoding`，**不引** `url` crate
- 混合 sources 支持：`copy smb://a /local/b mtp://c adb:///sdcard/d -o /x`
- **IPv6 host 用方括号**：`smb://[::1]/share` / `smb://user@[2001:db8::1]:445/share`；`split_host_port` 优先识别 `[...]`，普通 `host:port` 用 `rsplit_once(':')` 避免端口被前段冒号撞

### 凭据
- SMB：`SMB_USER` 经 `backend.smb.default_user` 兜底；`SMB_PASSWORD` 在 `build_target` 处读 env；Kerberos 走 `KRB5CCNAME`；`backend.smb.workgroup` 默认 `WORKGROUP`。**密码永远不入 YAML**
- ADB：走本机 `adb` daemon（adb_client 3.2 经 TCP 连 `127.0.0.1:5037`）；运行前需 `adb start-server`、设备开 USB 调试 + 文件传输；多设备 URI 必须带 serial

### 工厂与注入
- `adapters::backend::factory::BackendFactory` trait + `DefaultBackendFactory` 按 `Location` 装配：Local 直给 `LocalBackend`；SMB/MTP/ADB 走 cfg-gated 分支；MTP 当前 stub 返 `Unsupported`
- feature off 返 `Unsupported "<scheme> backend not enabled; rebuild with --features <scheme>-backend"`
- 测试侧 `tidy_with(factory, command)` 接 `BackendFactory` 注入；集成测试用 `FakeBackendFactory`；`FakeBackend` / `FakeOp` 已 `#[doc(hidden)] pub use` 到 crate 根

### 远端 backend 测试套路（SMB / MTP / ADB 通用）
- **手写 Fake\<Smb|Mtp|Adb\>Client**：state 用 `Arc<Mutex<HashMap<...>>>`；`inject(*Op::Read, path, ErrorKind::TimedOut)` 注入逐 op + 逐 path 错误，无须 `mockall`
- 错误文案映射：SMB / ADB `map_error` MUST 同时支持 **`NotFound`**（`enoent` / `no such file` / `does not exist`）+ **`PermissionDenied`**（`eacces` / `permission`）两类 ASCII 文案重映射，否则真实远端 `io::Error::other("pavao: ENOENT ...")` 让 `mkdir_recursive` 的 `NotFound` guard 永不触发 → 多层归档目录创建必败。**Fake 干净 `ErrorKind::NotFound` 会掩盖该缺陷**——专测 MUST 用 `FakeRemoteClient::with_error_factory(|k| match k { NotFound => Error::other("pavao: ENOENT ...") })` 模拟生产文案；匹配 MUST `to_ascii_lowercase()` 而非 `to_lowercase()`（避免 Unicode full-case folding 在本地化消息上字节漂移）
- **`map_error` 不能仅判 `kind == Other`**：`adb_client` 等链式错误可能返 `BrokenPipe`/`ConnectionReset` + 文案含 "no such file"；MUST 先放行已正确分类的 kind 再按文案重映射其余，否则 `exists()` 把"找不到"当 IO 错误传播
- **远端 helper 内调 `client.stat/mkdir/...` 也要 `map_err(A::map_error)`**：错误分类仅靠 `RemoteBackend::map_and_log` 单点不够——`mkdir_recursive` 等 trait 方法外的 raw client 调用未映射，pavao 把"路径不存在"包成 `Other("no such file")` 让 `e.kind() == NotFound` 守卫永远 miss
- 真实 client 适配器（`*_real.rs`）走 cargo-llvm-cov `--ignore-filename-regex='_real\.rs$'` 排除整文件（需真实服务/设备无法 CI 触发）；命名约定下 `factory_real.rs` 也被同 regex 排除；调度逻辑（`build_target` / `map_*_error` / `tidy_with` 各分支）走 fake 注入 100% 覆盖
- 每方法测三类：OK / client Err / 非自家 scheme 返 `InvalidInput`
- **"未启用 feature 返 Unsupported" 集成测试 MUST `#[cfg(not(feature = "<scheme>-backend"))]` gate**，否则启用 feature 跑 nextest 会 fail
- 远端 op 失败的结构化日志统一在 `remote.rs::map_and_log`（非泛型单点，避免 monomorphization 覆盖率重复计数）；新增远端日志沿用此单点，**勿**在 smb/adb/mtp 各自加

## 配置与日志
- 运行时配置：`config.yaml`（项目根）+ `src/usecases/config.rs`，`config()` 返 `&'static Config`（`OnceLock`）
- 切换 `TIDYMEDIA_CONFIG=/path/to.yaml`；`${VAR:-default}` 由 `expand_env` 自实现（不引 dotenv）；嵌套 `{}` 默认值按括号配对解析；展开后以 `{` 开头的值（如 `archive_template`）yaml 中 MUST 加引号，否则被当 flow mapping 致整个配置回退默认
- 非法值校验走 `frameworks/config.rs::sanitize`（warn + 回退默认，与 parse 失败回退 `Config::default` 同哲学；已覆盖 `unique_name_max_attempts==0`、非法 `archive_template`、非法 `log.level`）；新增可校验字段在此加分支，勿在消费点散写
- 结构化日志字段约定：`feature` / `operation` / `result`（CLI 无 request_id/user_id）
- `UtcOffset::from_whole_seconds` 范围 ±25:59:59，越界 None，用 `.unwrap_or(UtcOffset::UTC)` 兜底
- **R1 外置**：`copy.{timezone_offset_hours, unique_name_max_attempts, archive_template}` / `exif.valid_date_time_secs` / `backend.smb.{default_user,workgroup}` / `backend.adb.{server_host,server_port}` / `log.level`（CLI `--log-level` 缺省值，优先级 `RUST_LOG` > flag > 配置）。**无消费点的字段勿先加占位**
- **不外置的合理例外**：算法常量（`EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS` / `MTIME_VS_P0_HINT_SECS`）/ 协议字面量（`IMG_` / `DSC_` / `Screenshot_` / `XMP_KEY`）/ 日志维度名 / lookup 表（`MONTH`）/ 流式哈希（`FAST_READ_SIZE`、`STREAM_CHUNK = 1 MiB`、`MIME_SNIFF_BYTES = 256`）
- `src/usecases/copy/ops.rs` 的 `println!("\"{}\"\t\"{}\"", src, dst)` 是 CLI 脚本可读输出，**不是** R3 日志路径，不要改成 tracing

## Android / 移动端（feature `android-app`）
- uniffi 0.31 proc-macro 模式：lib.rs 顶层 `uniffi::setup_scaffolding!()` + `#[uniffi::export]` / `#[derive(uniffi::Record)]` / `#[derive(uniffi::Error)]`；不需要 build.rs / .udl
- **`#[derive(uniffi::Error)]` 字段名不能叫 `message`**：uniffi 生成 `class Generic(val message: String)` 与 `kotlin.Exception.message` 撞名编译失败；用 `text`/`detail`。同类 Throwable getter 名（`cause` 等）也避开
- `[lib] crate-type = ["rlib"]`：rlib 让桌面集成测试链得上；cdylib（给 Android JVM dlopen）**不写死 Cargo.toml**——Windows 上与 bin 的 tidymedia.pdb 同名冲突（cargo#6313），交叉编译时 `cargo rustc --crate-type cdylib` 按需覆盖；不要换 staticlib
- 交叉编译：`cargo ndk -t aarch64-linux-android -p 30 --output-dir mobile/android/app/src/main/jniLibs rustc --lib --crate-type cdylib --release --features android-app`
- Kotlin 绑定：`uniffi-bindgen generate --library <libtidymedia.so> --language kotlin --out-dir <dir>`；**`uniffi --features cli` 装出的 binary 实际叫 `uniffi-bindgen`**
- `src/frameworks/mobile.rs` 无独立 use case：直接 `tidy_with(&DefaultBackendFactory, Commands::Copy {..})` 复用 CLI 路径（YAGNI）；feature off 时 cfg 排除
- **`tidy_with` 单一入口返 `CommandResult` enum**（`Copy(CopyReport)` / `Find(FindReport)`）：CLI 走 `tidy(..).map(|_|())` 丢弃，mobile/Android 直接 `match` 取 report；**MUST NOT** 新增 `*_report()` 专用包装复制 dispatch 逻辑
- **mobile FFI 嵌套集合 MUST `Vec<Record>`**（如 `Vec<MobileDuplicateGroup>`），禁 `paths.join(",")` CSV——路径含逗号会被 Kotlin `split(",")` 拆错
- 实测工具链：JDK 25 (Temurin) + Gradle 9.1 + AGP 8.10 + Kotlin 2.0.21 + NDK r26d + SDK android-35（AGP 8.7 不支持 JDK 25）。`ANDROID_HOME ≠ ANDROID_NDK_HOME`：cargo-ndk 只读 NDK，Gradle build 还需 SDK，两者都要设

## 项目 Gotcha
- **测试 Windows 可移植**：Unix-only API（`PermissionsExt`/`OsStringExt`/`UnixListener`/chmod 类）测试 MUST `#[cfg(unix)]` gate、import 放函数内；路径前缀比较 MUST 用 `file_info::full_path` 规范化，MUST NOT 手动 `canonicalize()`——用户目录为符号链接时（`C:\Users\x` → `D:\Users\x`）canonicalize 解析出不同盘符与索引不匹配
- Cargo.toml 多数 dep 用 `"*"` 通配；`cargo update` 可能拉到不兼容主版本（sha2 0.10→0.11 已踩坑），主版本升级前先 dry-run
- **本地 `[patch.crates-io]` 验证依赖 patch**：`cp -r ~/.cargo/registry/src/<idx>/<crate>-<ver>/ <vendor>` + patch + `[patch.crates-io] <crate> = { path = "..." }` → `cargo build` 触发重编。**Cargo.lock 中 patched crate 无 `source =`/`checksum =`** 即生效证据
- **远端 `RemoteClient::read` 整文件入堆**（已知限制，trait 文档有 WHY）：pavao `SmbFile` 借用 client 生命周期装不进 `Box<dyn MediaReader + 'static>`、`adb_client` pull 是回调式写入 API——真正流式需换库或线程+管道，YAGNI；大视频在内存受限环境（Android）有 OOM 风险
- **远端 `mkdir_p` 是真递归**（`remote.rs::mkdir_recursive`）：自底向上 stat 找存在的祖先再逐层 mkdir（`AlreadyExists` 容忍、stat 非 NotFound 直接传播）。`FakeRemoteClient::mkdir` 不校验父目录，单层 vs 递归差异 fake 测不出来，须用「中间层注入错误/断言中间层 metadata」类测试钉行为
- **存在性查询 Err MUST 传播，MUST NOT `unwrap_or(false)`**：吞 Err 让后续 `open_write` truncate 覆盖已有文件、move 模式连源一起删；保守兜底方向是 `true` 而非 `false`。**`std::path::Path::exists` / `camino::Utf8Path::exists` 返 `bool`、内部吞 `PermissionDenied` 成 false 是同类陷阱**——`LocalBackend::exists` MUST 用 `path.as_std_path().try_exists()?` 让 Err 经 `?` 上抛
- **测试 shim 必须 `#[cfg(test)]` gate**：`Info::from` / `Index::visit_dir` / `Exif::from_path_with_offset` / `adapters/backend/fake_remote` 是包 backend-aware API 的旧入口，仅测试用；未 gate 让 release build 报 `dead_code`
- **`#[cfg(test)]` 标在方法/import 上，不要标在 `impl Foo {}` 块上**：同块生产方法会被一起 gate 掉
- **`--all-features` clippy 与 `#[cfg(not(feature))]` test 联动**：启用全 feature 后，gate 掉的 test fn 对应 imports 必须用同样 `#[cfg(not(all(feature = "smb-backend", feature = "mtp-backend", feature = "adb-backend")))]` 包裹，否则 `unused_import` error
- **clippy 1.95 `doc_markdown` 扩大**：含点号的文件名、含下划线的标识符在 `///`/`//!` 中均需反引号包
- **`chrono::TimeDelta::seconds(i64)` 会 panic**（secs > ≈ `i64::MAX/1000`），`?` / `.ok()?` 截不住——外部 timestamp（Takeout JSON / 用户输入）解析 MUST 用 `try_seconds()?` + `DateTime::checked_add_signed`
- **重复组容器 MUST `Vec<DuplicateGroup { size, paths }>`**，MUST NOT `BTreeMap<size, _>`：size 作唯一键让同 size 不同 content 的两组互相覆盖
- **路径前缀匹配 MUST 校验分隔符边界**：`"/photos_backup/x".starts_with("/photos")` 误判 → `find.rs::under_prefix` 已封装。**prefix 末尾 `/`/`\\` 在 helper 内剥**：朴素 `starts_with` 会让 `rest='img.jpg'` 既非空也不以分隔符开头而误判
- **含 secret 字段的 struct MUST 手写 `Debug`**：derive 让 `debug!(target=?t)` 把明文写日志（P0 §12 违反）；手写把 secret 渲染成 `Option<&"***">` 占位（参考 `adapters/backend/smb.rs::SmbTarget`）
- **外部 JSON schema 同一字段历史可能 String 或 Number**：Google Takeout `photoTakenTime.timestamp` 多版本类型不同，单字段严格 `String` 让 P3 候选静默丢失；用 `serde_json::Value` + 二态 match 比 `deserialize_with` 简洁
- **`ReportSink` trait 用 `enum Report<'a>` + 单方法 `write(&Report<'_>)`** 收敛多报告类型；对象安全 + 新增 report 变体不强制升级既有 impl
- **`Info::cloned_at(loc, backend)` 复用 src hash 入 `output_index`**：copy/move 完成对 dst 重新 `Info::open` 既浪费（远端额外 RTT + 4 KiB 读）又在 NFS ESTALE / 防病毒抢占下让 dst 已写未入索引 → 后续同 hash 源 unique-name 再写一份成重复副本
- **跨设备 `fs::rename` fallback 是 copy+remove 非原子**：copy 成功 remove 失败的半态 Err MUST 文案含 `copied ... but cannot remove source`，让 `do_copy` / failed 计数与 "copy 也失败" 场景区分；否则用户误判后重跑会把已被 copy 的源也删
- **跨 scheme sibling 路径用 `Location::with_path`/`path` 单点**：sidecar `with_extension` / `append_suffix` 不必各 backend 各实现——远端 backend 通过 `Location` 自然等价
- **`use rayon::prelude::*` 违反 P0 §5**：最小 import = `use rayon::iter::{IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator}`（`par_iter_mut` 需 RefMut，少了编译失败）
- **同盘 move fast-path**：`do_copy` 在 `opts.remove && src.scheme()=="local" && output.scheme()=="local"` 时走 `Backend::rename`（`LocalBackend::rename` 内 `fs::rename` 同卷原子 + `CrossesDevices` fallback 到 `fs::copy + remove`）。**MUST NOT** 在 Rust 层用 `metadata().dev()` / `GetVolumeInformationByHandleW` 重复判同卷——OS 内核是 same-volume 唯一权威源
- **dry-run log / 路径聚合用 Python**（heredoc 或 `uv run xxx.py`）：tidy-verify Step 3/4 已从 awk 迁到 `.claude/scripts/tidy-verify/*.py`（避 awk `\` regex 在 bash 单引号下二次解析致 `unterminated regexp`）；临时一次性分析用 `cat > /tmp/tm/x.py <<'PYEOF' ... PYEOF` + `cd /tmp/tm && uv run x.py`。**Windows 输出陷阱**：Python stdout 默认 CRLF 与下游 grep/diff LF 不一致 MUST `sys.stdout.reconfigure(newline='\n')`；终端 stdout 默认 GBK 把 UTF-8 中文显示成 `��` → 含中文/路径的报告 MUST 写 UTF-8 文件再读；Write 工具落盘含 `'\\'` 的 Python 字符串字面量会吞反斜杠（用 `SEP = chr(92)` 或 heredoc 绕开）；Python `re` alternation 是 leftmost-first 不是 POSIX leftmost-longest，迁 awk regex MUST 把长 token 放前（`(1[012]|0?[1-9])` 而非 `(0?[1-9]|1[012])`）
- **find 删除脚本统一 Python 输出**：`render_script` 输出 `#!/usr/bin/env python3` shebang + `import os` 头 + `os.remove("...")` 待删 / `# os.remove("...")` 注释保护，按 `DuplicateGroup` size 降序分组。**路径转义**：`s.replace('\\', "\\\\").replace('"', "\\\"")`；含 `\n` 控制字符暂不处理（YAGNI）
