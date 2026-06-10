@~/.claude/rust-p0.md

@~/.claude/rust-p1.md

# tidymedia 开发上下文

按「拍摄时间」去重并整理照片/视频的多后端 CLI：扫描 sources（local/smb/adb/mtp 可混合）→ SHA-512 去重 → 按解析出的拍摄时间归档到 `output/年/月`。核心算法 = 拍摄时间判定（P0–P4 优先级，见下文「核心算法」节）。Clean Architecture 四层 + Android app（feature `android-app`）。代码注释里的 `docs/media-time-detection.md §X` 指向已删除的 spec，待补。

## Quick Start
- **所有 cargo 命令（build/run/nextest/clippy/llvm-cov…）MUST 带 `--release`**：`profile.release` 已设 `opt-level = 0`，编译速度与 debug 相当，统一用一个 target 目录避免双份编译产物
- MSVC 链接已固化在 `.cargo/config.toml`（linker 绝对路径 + `[env] LIB`；PATH 中 GNU coreutils `link` 会抢占 `link.exe`、vcvars64 在精简 shell 环境下不可用），cargo 直接用；换 MSVC/Windows SDK 版本需同步更新该文件
- 构建：`cargo build --release`；运行：`cargo run --release -- copy /source -o /output`；dry-run：`cargo run --release -- copy /source -o /output --dry-run`
- **CLI flag 位置**：`--log-level=debug`（带连字符）是 **全局** flag 放最前；`--dry-run` 是 **子命令级** MUST 放 `copy`/`move` 后；tidymedia debug 走 stderr，`| tail -N` 会截 copy_file 行让 summary `copied=N` 与日志行数对不上 → 重定向到文件再 grep
- 测试：`cargo nextest run --release`；覆盖率：`cargo llvm-cov --release nextest --summary-only`（stable 够用；nightly 严格 100% 口径见「测试与覆盖率」节）
- lint：`cargo +nightly fmt && cargo clippy --release --all-targets --all-features --locked -- -D warnings`（默认与 `--all-features` 均 0 warning；**`--all-features` 仅 Linux 可验**——smb-backend→pavao-sys 需 libsmbclient，Windows 编不过，本机验默认 feature 即可）
- rustup default-host 与全部工具链 MUST 用 `x86_64-pc-windows-msvc`（`+nightly` 按 default-host 解析；gnu 工具链与 MSVC 链接不兼容）；cargo-llvm-cov 安装不带 `--locked`
- git commit 格式：`type: 中文描述`（本项目无 TASK-ID 前缀），按主题拆分提交

## 系统依赖与库特性
- 无外部进程依赖；EXIF/视频元数据走 `nom-exif`（图片+视频）+ `infer`（magic-bytes MIME）
- `bin/exiftool/exiftool.exe`（仅开发调试，非运行时依赖）：排查/修复 EXIF 时间，如 `-time:all -s <file>` 查全部时间字段、`-P '-AllDates<Filename' -overwrite_original <dir>` 按文件名批量改时间且保 mtime
  - **中文路径**：本机 portable exiftool 缺 `Win32API::File`，命令行直接传 UTF-8 路径会 `Invalid filename encoding` + 0 文件，`-charset FileName=GBK` 也告警；省掉 `-charset` 让 exiftool 走 ANSI 系统调用反而最稳（stdout 显示乱码不影响 EXIF 抽取）；要 UTF-8 路径必须仿早期 tidymedia（7ea18d7 前）走 **tempfile + `-@ <file>` + `-charset filename=UTF8`**（小写 `filename`，路径写文件而非命令行）；`"-AllDates+=Y:M:D H:M:S"` 日期平移；`"-FileModifyDate<DateTimeOriginal"` mtime 同步 EXIF（对 AVI 等不可写格式也有效）；单文件按值写 `-P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=..."` 让 tidymedia 走 P0
  - 写操作默认产生 `<file>_original` 备份，处理完 MUST 清理——magic-bytes 仍是 JPEG，会被 tidymedia 当媒体文件归档
  - **`-T` Windows 输出末字段带 `\r`**：exiftool `-T` 在 Windows 走 CRLF 行尾，下游 awk `$NF` 拼出来含 `value\r`；终端 `\r` 回车把后续字段盖回行首致 print 错乱、字符串比较 `$NF=="value"` 永假；awk 处理 MUST 在 pass 入口 `sub(/\r$/, "", $NF)` 剥离（参考 `.claude/scripts/tidy-verify/compare_buckets.awk`）
- nom-exif 内部用 `tracing::info!/warn!` 大量输出，`install_logging` 用 EnvFilter 把 `nom_exif=error` 默认压住，保留 `RUST_LOG` 覆盖
- nom-exif 3.5 把 MKV `DateUTC` 合并到 `TrackInfoTag::CreateDate`（无独立 tag）；区分 MP4/MOV vs MKV/WebM 需 MIME 嗅探（`video/x-matroska` / `video/webm`）分流 `Source::MkvDateUtc` vs `QuickTimeCreateDate`
- nom-exif `Exif::get(tag)` 仅读 IFD0/MAIN；GPS 子 IFD 标签必须用 `Exif::iter()` 按 tag code 匹配
- **老 QuickTime `pnot`/`mdat` 起头 MOV**（NIKON COOLPIX S5/P5000、Casio 等 2003-2008 早期机型，及无任何头 atom、`moov` 在文件末尾的 mdat-first 变体）nom-exif 3.6.0 `parse_bmff_mime` 拒识：只认 `ftyp`/`wide` 首 box，`HEADER_PARSE_BUF_SIZE=128B` 也扫不到 `mdat` 兜底。当前 fork patch 在入口短路（首 box header==pnot|mdat 直返 QuickTime mime；**必须 `BoxHeader::parse` 仅解析 header**——`BoxHolder::parse` 要求整 box body 在 buffer 内，数 MB 的 mdat 必 Incomplete 让短路静默失效），`Cargo.toml [patch.crates-io] nom-exif = { git = "https://github.com/enjoypi/nom-exif.git", branch = "feat/legacy-quicktime-pnot" }`；后续 `mov.rs::extract_moov_body_from_buf` 经 `ClearAndSkip` 跳过任意大 mdat 解析 `mvhd.creation_time`，与 ExifTool 同源，无需自写 quicktime.rs。fork 本地 clone：`D:/prj/nom-exif-git`（无 `.cargo/config.toml`，构建前从 tidymedia 复制 MSVC 链接配置，已加 `.git/info/exclude`）。上游 PR 合并发版后切回 `*` 删 patch。mdat-first 真实样本全库已绝迹，可从 pnot MOV 剥头合成：先读各 box offset，`tail -c +<pnot+PICT 总长+1>` 即得 mdat 起头 + moov 在末尾的等价文件
- nom-exif trace 调试：`RUST_LOG=nom_exif=trace`；trace span 名是 `#[tracing::instrument]` 函数名嵌套（`parse_bmff_mime:parse: Got box_type=...` 中 `parse` 是 `BoxHolder::parse` 子 span）
- **AVI（RIFF）nom-exif 不支持**：老相机（Fujifilm `FinePix` 等）AVI 的拍摄时间在 `LIST hdrl > LIST strl > strd` chunk（`AVIF` 魔数 + 裸 TIFF IFD，offset 基准 = strd+8），由 `entities/riff.rs` 自解析填入 `Exif` 的 `date_time_original`/`create_date`/`make`/`model`（P0/P1 候选 + `{make}/{model}` 复用既有链路，无新增 `Source`）；`exif.rs::from_reader` 按 `video/x-msvideo` 先于泛 video 分流
- **M2TS/AVCHD（BDAV MPEG-TS）nom-exif 不支持**：Canon 等摄像机拍摄时间在 H.264 SEI `user_data_unregistered`（MDPM UUID，exiftool 读作 `H264:DateTimeOriginal`），由 `entities/m2ts.rs` 自解析——按 AVCHD 固定 video PID 0x1011 重组 192B BDAV 包 payload 后直搜 UUID（不解析 PMT/NAL，YAGNI；PID 重组天然解决 UUID 跨包切断），EP 字节（`00 00 03`）按 RBSP 规范剥离（午夜 00:00:00 BCD 会触发）；MDPM 布局已对真实文件逐字节对账 exiftool：tag `0x18`=[时区,世纪,年,月] BCD、`0x19`=[日,时,分,秒] BCD，时区字节忽略走 naive+配置时区口径；仅填 `date_time_original`（单一拍摄时刻不伪造 P1）；`from_reader` 按 `video/m2ts` 先于泛 video 分流；fixture `tests/data/sample-canon-avchd.m2ts`（真实 Canon 截前 1024B）
- **JPEG/HEIC/TIFF XMP packet 单段 fallback**：相机原拍后被 Lightroom/Bridge/ACR re-tag 的图，EXIF IFD0 可能仅剩 `ModifyDate`，原始拍摄时间落在 XMP `<x:xmpmeta>` packet 的 `photoshop:DateCreated`（→ DTO/P0）与 `xmp:CreateDate`（→ CreateDate/P1）。`populate_image_dates` 在 EXIF DTO+CreateDate 双 0 时扫已 buffer 头部 64 KB（`XMP_SCAN_BYTES`），由 `entities/xmp.rs` 抽 attribute 形式（单/双引号都支持，element 形式 YAGNI、ExtendedXMP YAGNI）；`adapters/sidecar.rs::parse_xmp_date` 复用同实现避免两份漂移。fixture 用 `bin/exiftool -api Compact="Shorthand,NoPadding"` 才出 attribute 形式（默认 element）。**seek 失败仍跑 XMP fallback**：已读入的 head 不浪费，否则非可寻址 MediaReader 的 re-tag 图退回 mtime 归错桶
- **XMP 未闭合 `<!--` 按 strict 语义抹至 EOF**（fixture `parse_xmp_date_unterminated_comment_none` 钉死）：lenient（视 `<!--` 为字面量）会把注释中的 `photoshop:DateCreated="OLD"` 误读为合法属性；XMP packet 是 well-formed XML 子集，未闭合注释属 malformed，宁可放弃后续也不冒误读风险

## 测试与覆盖率（项目特有；通用套路见 rust-p1 §5）
- **覆盖率门槛：region / line / branch 三项 MUST 全 100%**（严格口径 + `--branch`，缺一不可）。**Windows 本机默认 feature 口径有既有基线 miss**（`uri.rs`/`sidecar.rs`/`filename.rs`/`resolve.rs`，多为 feature-gated 分支与 multi-binary instance）：本机判定标准 = 新增代码全 100% 且 TOTAL miss 与 main 基线持平；怀疑回归先 `git stash` 跑基线对照再排查
- 严格 100%：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch`；`lib.rs` / `bin/tidymedia.rs` 顶部 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]`；`Cargo.toml [lints.rust] unexpected_cfgs` 注册 `cfg(coverage_nightly)`；不可稳定触发分支用函数级 `#[cfg_attr(coverage_nightly, coverage(off))]`
- **`--branch` multi-binary instance 陷阱**：lib unit + 集成 binary 各自 codegen 热点 fn 副本，每副本独立计数器；某 binary 未触发即报 instance miss。可行：①重构成 `?`（算 region 不算 branch）；②函数级 `coverage(off)`（用于 hash / file_info / copy / exif 等 hot fn，具体位点 `rg "coverage\(off\)" src/`）
- 改 `Cargo.toml` / `coverage` 属性后必跑 `cargo +nightly llvm-cov clean --workspace`
- `FakeBackend::inject_reader_error`：`open_read` 成功但 reader `read` 立即 Err，覆盖 stream hash / `sniff_mime` 等 `?` Err 分支；`FakeBackend::add_file_with_times` 构造 `modified=None`/任意 `created`（真实 fs 造不出 mtime 缺失，`create_time` 边界用）；`DummyClient.mkdir_calls` 原子计数杀「best-effort 调用被替换成 no-op」类变异
- **fast-path 命中反向证据**：`fs::rename` / `fs::copy` 保留 src mtime，`std::io::copy` 不保留；用 `filetime::set_file_mtime` 钉 src 后 move 断言 dst mtime 不变即证明 fast-path 命中（参考 `tests/lib_tidy/move_idempotency.rs::move_local_fastpath_dst_mtime_matches_src`）。fast-path 走 `fs::rename` 不读源 → "chmod 000 src 文件" 类失败测试失效，改为 chmod 555 **src 父目录**（rename 需父目录 write 权限）
- **Windows 同卷边界本地测**（`tests/lib_tidy/windows_same_volume.rs`，`#![cfg(windows)]`）：`Command::new("subst").args([letter, tempdir])` + `Command::new("cmd").args(["/C","mklink","/J",link,target])` 构造「不同前缀同卷」，**均不要 admin**；subst 盘字全局共享 → `.config/nextest.toml` 加 `[test-groups.windows-volume-mut] max-threads=1` + `filter='test(/windows_same_volume::/)'` 强制串行；释放盘字用 RAII Drop guard `subst Y: /D`
- **`--all-features` 下 `dispatch.rs` 的 `usecases::copy(..)?` Err arm 覆盖**：现有 `tidy_rejects_*` 测试用 `#[cfg(not(feature = "smb-backend"))]` gate，启用全部 feature 跑覆盖率时这些测试不编译 → `?` Err arm miss。用 output 父路径占成普通文件触发 `mkdir_p` Err 兜底（参考 `dispatch_and_cli.rs::tidy_copy_propagates_mkdir_error_when_output_parent_is_file`）
- **`--all-features` 严格覆盖也须 100%**：feature 启用侧用 `#[cfg(feature = "...")]` gated 测试镜像 `tidy_rejects_*` 系列（`tests/lib_tidy/real_factory.rs`）。标准杠杆：`RealMtpClient::new` 是 stub 必 Err → 确定性触发 `dispatch.rs` 各 `?` Err arm 与 `factory.rs` builder 调用位点；`PavaoClient::new` / `ADBServerDevice::new` 仅初始化不连网可直接断言。`factory.rs::unsupported_backend` 三 feature 全开时 dead → 条件豁免 `cfg_attr(all(coverage_nightly, 三feature), coverage(off))`（与 `allow(dead_code)` 条件镜像）。`mobile.rs` 不可达防御逻辑抽纯 helper（`expect_copy`/`expect_find`/`copy_status`/`stats_from`/`mobile_report_from`）直测；调用点用 `.map` 替代 `?` 消除永不触发的 Err 传播 region
- **子行 region miss 定位**（lcov `DA`/`BRDA` 不显示，如 `?` Err arm）：`cargo +nightly llvm-cov report --release --text` 复用上次 run 的 profdata（不接 `--all-features`，feature 集随上次 run），输出中 `^0` 标记即未覆盖子行 region
- **branch miss 精确定位**：`llvm-cov report --release --lcov` 过滤 `BRDA` 行第 4 字段为 0（`--text` 的 `^0` 只标 region，branch 看不到）
- **逻辑不可达的 `?` 死区消除**：循环边界用 `while let Some(x) = buf.get(start..end)` 替代手写 `off + N <= len` 哨兵（参考 `entities/riff.rs`）；`buf.get(idx..)?` / `s.get(idx..)?` 当 idx 来自 `windows().position()` / `str::find()` 时改 `&buf[idx..]` 直接索引（position/find 保证 idx ≤ len，`?` Err arm 不可达，参考 `entities/xmp.rs::find_xmp_packet` / `find_attr_rfc3339`）；纯类型安抚用 `unwrap_or(常量)` 替代 `.ok()?`
- **`Cursor` 的 seek 永不失败**：测 seek Err 传播分支需手写 FailSeek reader（包 Cursor，read 透传、seek 恒 Err，blanket impl 自动满足 `MediaReader`）
- **mutation testing**：配置在 `.cargo/mutants.toml`（nextest + release + `--all-features`，排除 `*_real.rs`/测试基建；等价/平台 cfg/feature-gate 豁免全在 `exclude_re` 逐条注明 WHY）。**Windows 本机跑不了 `--all-features`**（smb→libsmbclient 仅 Linux）：增量验证临时注释 `additional_cargo_args` 按默认 features 跑，feature-gated mutants 留 Linux 全量审计。日常增量 `cargo mutants --in-diff <(git diff main)`；全量审计 677 mutants / 约 3h（2 核机；2026-06 基线：509 caught / 168 unviable / 0 missed）；定点复验 `cargo mutants -F '<name regex>'`（注意 `-E` 是 exclude）。已沉淀套路：①计数断言 MUST 精确 `==`，`>= 1` 杀不掉 `+=`→`-=`（usize release wrap 成 MAX）；②与被调函数重复的入参 guard 产生等价变异，删冗余 guard 单点校验（DRY）；③`cfg(windows)`/`cfg(not(feature))` 代码在单口径跑必假幸存 → exclude_re；④只进 tracing 字段的取值（如 `summary_result`）抽纯 fn 直测，不靠捕日志；⑤防御性不可达 arm 喂错配变体直测（见上文「`--all-features` 严格覆盖」条的 `mobile.rs` helper 模式）；⑥写 kill 测试前先确认变异行在该输入下**可达**（`create_time` 无 EXIF 时 let-else 早返回，到不了被变异行——曾致 kill 测试绿但 mutant 仍幸存）；⑦`cargo mutants` 启动即快照源树，长跑期间可安全并行写 kill 测试/改源码，不影响进行中的运行；⑧纯移动重构的 `--in-diff` 是假增量（移动行全算 changed → 接近模块全量，105 mutants ≈ 32min），可缩小 diff 到真改逻辑的文件，或信任「移动前 0 missed + 移动后测试全绿」跳过；⑨无返回值的纯 tracing 副作用 fn（如 `log_parse_failure`）`replace X with ()` 等价幸存（规约不捕日志断言）→ exclude_re 豁免并注明 WHY；⑩长跑期间 `mutants.out/*.txt` 增量写入，过早读取会误判已完成——结束判定以 stdout `N mutants tested in X` summary 行为准

## Fixture
- `tests/data/` 下文件 mtime 每次 `git checkout` 重置；时间相关测试 **MUST** 用 `filetime::set_file_mtime` 固定（封装：`entities/test_common::copy_png_to`，固定到 `FIXED_MEDIA_MTIME` = 2024-01-01 12:00:00 UTC）
- MP4 不传 `-metadata creation_time=` 时 nom-exif 返 `Some(1904-01-01)`（QuickTime epoch），要 None 用 MKV
- `FakeBackend` 默认 mtime = `UNIX_EPOCH` → `create_time` P4 兜底 + 默认时区 +8 → copy/move e2e 的归档桶固定为 `1970/01`（断言目标路径时直接用）
- `camino::Utf8Path` 在 Linux 上**不**把 `\` 当分隔符，Windows 反斜杠路径测试行为不同

## 文件组织
- 文件 ≥400 行预防性拆分至 ≤300：测试文件按先例追加第二个 `#[cfg(test)] #[path = "X_yyy_tests.rs"] mod yyy_tests;`；生产文件目录化 `foo/mod.rs`（仅声明+re-export，rust-p0 §9）+ 子模块（参考 `entities/file_info/`、`usecases/copy/`）。测试文件随迁目录、`#[path]` 挂 mod.rs；测试要访问的内部项在 mod.rs 用 `#[cfg(test)] use self::sub::item;` 私有 re-export（私有 use 对子模块可见，`super::X` / `super::super::*` glob 照常解析）；跨子模块项可见性用 `pub(super)`，对外 API 用 pub-in-private-mod + `pub(crate) use`。子模块 **MUST NOT** 命名 `core`（与内建 crate 撞名致 `use core::` 歧义）；仅测试消费的 re-export 触发 `unused_imports` → 改 `#[cfg(test)] use`
- CA 重构移动文件：`pub use A::B` 只 re-export 类型不保留路径；`pub mod B { pub use A::B::*; }` 保留完整模块路径；配套测试 `super::` 需改 `crate::` 绝对路径
- CA 依赖方向验证：`rg "use crate::adapters|use crate::frameworks" src/entities/ src/usecases/` 应仅返回 re-export 桥接
- 集成测试拆分：`tests/<name>.rs` 是 root binary，`tests/<name>/*.rs` 子目录 **不**会被当独立 binary；root 用 `#[path = "<name>/sub.rs"] mod sub;` 装配（参考 `tests/lib_tidy.rs`）

## 同步检查点（改 X → MUST 同步 Y）
> 字面默认值变更先 `rg <旧值>` 兜底改全。

- **新增 `Location` variant / backend scheme** → `entities/uri.rs` 的 `FromStr` + `adapters/backend/factory.rs`（cfg-gated 分支 + Unsupported 兜底）+ 对应 `Backend` 实现 + `adapters/dispatch.rs` 调度 + 本文「URI 格式」节
- **新增 `Backend` trait 方法** → 全部 7 个实现同步加默认或 override：`local`/`remote`/`smb`/`adb`/`mtp`/`fake`/`fake_remote`；按「远端测试套路」补 OK / client Err 注入 / 非自家 scheme 三类测试
- **新增配置字段** → `usecases/config.rs` 结构体 + `config.yaml` + `validate_*` 校验或被消费（杜绝哑配置）；secret 再加 `.env.example`（值 `changeme`）+ 确认 `.env` 已 gitignore
- **新增 CLI flag** → `adapters/dispatch.rs` 调度透传 + **每个子命令路径（copy/move/find）独立 e2e 触发 Some/None 两边**，否则 LLVM branch miss；e2e **MUST 含 `run_cli(["tidymedia", ...])` 字符串形式**（clap flag 名映射只有该形式能验证，仅 `tidy(Commands::..)` 不算完整，参考 `tests/lib_tidy/run_cli_flags.rs`）
- **新增 `media_time` 候选来源 / 调整 P0–P4** → `entities/media_time/priority.rs` 的 `Source` + `Priority` 枚举 → 对应解析模块（`entities/exif`、`entities/media_time/filename`、`adapters/sidecar`、`entities/media_time/fs_time`）→ `resolve`/`decision` 裁决 → 补 fixture
- **新增 archive_template 占位符** → `archive_template.rs::render` 替换分支 + 同文件 `PLACEHOLDERS` 常量 + `config.rs::validate_archive_template` 测试三处同步

## 项目分层（Clean Architecture）
- 四层（自外向内）：`src/frameworks/` → `src/adapters/` → `src/usecases/` → `src/entities/`
- `bin/tidymedia.rs` **只**调 `tidymedia::run_cli(env::args_os())`，零业务逻辑；`lib.rs` 仅模块声明 + re-export
- `usecases/` 仅依赖 `entities/`；通过 `pub use crate::frameworks::config::config;` re-export 让 usecases 内部用 `super::config::config` 不直接依赖 frameworks
- `entities/backend/` 是 Gateway 抽象（`trait Backend` + `SmbTarget` / `AdbTarget` / `MtpTarget`）；具体实现在 `adapters/backend/` 通过 re-export 模块保持原有路径；`file_info` / `file_index` / `exif` / `media_time::sidecar` 都 backend-aware（持 `Arc<dyn Backend>`）
- 目录名是 `usecases`（无下划线）

## 核心算法：media_time
- 优先级：P0 = `ExifDateTimeOriginal` / `QuickTimeCreationDate` / `MkvDateUtc`；P1 = `ExifCreateDate` / `QuickTimeCreateDate`；P2 = 文件名启发式；P3 = `XmpSidecar` / `GoogleTakeoutJson`；P4 = `FsMtime`。mtime 比 P0 早 > 30 天发提示性冲突告警；**多数派仲裁**：filename 候选与 mtime 互证（差≤1天）且与 P0 差>30天 → 推翻 P0（相机时钟错场景，`ConflictKind::P0OverruledByMajority` 记冲突告警不静默）；**ModifyDate 三方互证否决**：互证成立但 filename 票与 EXIF `ModifyDate` 差≤1天 → 判定 filename+mtime 为 re-save 时戳（第三方批量 re-save 保留 DTO 场景），保 P0 记 `MajorityVetoedByModifyDate`（`ModifyDate` 解析进 `Exif.modify_date` 但不进时间候选，仅作仲裁旁证）
- `entities/media_time/` 7 子模块单一职责：`priority`（`Source` + `Priority` 枚举）/ `candidate`（`epoch_to_candidate`：secs==0 视为未填返 None）/ `filename`（P2 启发式：`IMG_`/`DSC_`/`Screenshot_` 前缀 + 13 位毫秒 Unix 戳 + WeChat/WhatsApp/Pixel/裸 15 字符 `YYYYMMDD_HHMMSS` + 通用 `<任意前缀>YYYY-MM-DD HH-MM-SS` 19 字节窗口 + **宽松 YYYYMMDD**（stem 开头或 `-`/`_`/空格 后 8 位合法日期，仅日期粒度兜 DSC_/PXL_ 严格模板坏掉但 stem 含合法 8 位日期场景））/ `filter`（合理性过滤 + `EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS`）/ `resolve` + `decision`（多候选裁决）/ `fs_time`（P4）
- P3 sidecar 在 `adapters/sidecar.rs`（Interface Adapter 层，不在 entities）：XMP `photoshop:DateCreated` 纯文本搜索 + Takeout `<media>.<ext>.json` `photoTakenTime.timestamp` 走 `serde_json`；backend-aware，sibling 路径计算当前仅 Local（SMB/ADB 上 sidecar 静默跳过）
- **P0–P4 全链已接线 `Info::create_time`**：P0/P1 来自 EXIF；P2 由 `candidates_from_filename` 直接消费（文件名 naive 时间按 `timezone_offset_hours` 配置时区解释，与 EXIF naive 同口径——曾按 UTC 解释致月末晚间文件 +8 跨月归错桶）；P3 经依赖倒置注入——`dispatch` 把 `adapters::sidecar::discover_with_backend` 传给 `usecases::copy_with_sidecar`，`Index::enrich_candidates` 并行调 provider 填 `Info::add_candidates`；P4 = mtime。**e2e fixture 注意**：含 P2 文件名模式（`IMG_*` 等）或带 `.json`/`.xmp` sidecar 的文件会按文件名/sidecar 时间归档，不再落 mtime 年月；`copy()`（无 provider）是 `#[cfg(test)]` shim，生产只走 `copy_with_sidecar`
- **EXIF vs 归档桶对账**：EXIF naive 时间按 `timezone_offset_hours`（默认 +8）转 epoch、归档再 `.to_offset(+8)` 取年月 → 首尾抵消，**归档桶 = EXIF 字符串前 7 字符 `YYYY:MM`**，对账无需算时区。全流程 slash command `/tidy-verify <source> <output>`（`.claude/commands/tidy-verify.md` + `.claude/scripts/tidy-verify/`）：dry-run → exiftool 抽 → 桶 MISMATCH → 文件名时间 DIFFER → exiftool 修 → 真跑 move
- **tidy-verify dry-run log 年月归位分析**：直接按 `source=`/`target=` 父目录聚合会把「source/output 根不同」误判成"全部变化"；MUST 剥源/目标根前缀到相对路径再比年月段（绝大多数文件年月一致只是顶层补全 `YYYY\` 段，实质换月/换年通常是少数视频被 EXIF/容器时间归到拍摄真月份）
- **相机出厂默认时间陷阱**：EXIF P0 格式合法但值为出厂默认（如 2004-01-01 00:00），且早于机型发布日（FinePix E550 = 2004-07、Casio EX-Z50 = 2004-09）即可断定时钟未设；mtime 通常同错（写卡时间），多数派仲裁救不了，只能 exiftool 数据侧修复后再归档
- **多数派仲裁仅认 `Validity::Valid` 候选**：`apply_filters` 保留 `LowConfidencePre1995` 进 surviving；`majority_override` MUST 显式 `matches!(v, Validity::Valid)` 过滤掉低置信票，否则 1995 前的 filename+mtime 互证可推翻可信 P0（扫描件被批量 touch 出错误年份的场景）
- **`Screenshot_` 截图两种主流命名**：`yyyy-mm-dd-HH-mm-ss`（19 字符，Windows Snip & Sketch）与 `yyyymmdd_HHMMSS`（15 字符，Samsung/MIUI/原生 Android），`try_screenshot` 都要支持，否则后者退到 `try_loose_yyyymmdd` 兜底会退化为日精度丢时分秒
- **copy/move 重叠保护**（`copy/run.rs`）：source ⊆ output（canonical 前缀含相等）→ InvalidInput 拒绝（move 会把源判为自身副本删除）；output ⊂ source（就地归档）→ `Index::remove_under_prefix` 把已归档文件从 source 索引剔除。前缀比较用 `entities::common::under_prefix`（分隔符边界），Local 走 `full_path`（绝对路径透传、相对路径才 canonicalize）、远端用 display

## URI 与 Backend

### URI 格式
- CLI `sources` / `output` 接 `Location`（实现 `FromStr` 让 clap 自动 value_parser）：
  - 无 `://` 或 `local://` ⇒ `Local`
  - `smb://[user@]host[:port]/share/path` ⇒ `Smb`
  - `mtp://device/storage/path` ⇒ `Mtp`
  - `adb://[serial]/abs/path` ⇒ `Adb`；serial 为空（`adb:///sdcard/...`）让 client autodetect；path 始终是设备绝对路径
- 字段内空格/中文/分隔符走 `percent-encoding`，**不引** `url` crate
- 混合 sources 已支持：`copy smb://a /local/b mtp://c adb:///sdcard/d -o /x` 合法
- **IPv6 host 用方括号包裹**：`smb://[::1]/share` / `smb://user@[2001:db8::1]:445/share`；`split_host_port` 优先识别 `[...]` 形态，普通 `host:port` 用 `rsplit_once(':')` 避免端口被前段冒号撞（朴素 `split_once(':')` 会把 `[::1]` 错拆成 host=`[` port=`:1]`）

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
- **`map_error` 不能仅判 `kind == Other`**：`adb_client` 等链式错误可能返 `BrokenPipe`/`ConnectionReset` + 文案含 "no such file"；MUST 先放行已正确分类的 kind（`NotFound`/`PermissionDenied`）再按文案重映射其余 kind，否则 `exists()` 会把"找不到"当 IO 错误传播
- **远端 helper 内调 `client.stat/mkdir/...` 也要 `map_err(A::map_error)`**：错误分类仅靠 `RemoteBackend::map_and_log` 单点不够——`mkdir_recursive` 等 trait 方法外的 helper 内的 raw client 调用未经映射时，pavao 把"路径不存在"包成 `Other("no such file")` 会让 `e.kind() == NotFound` 守卫永远 miss，多层 `{year}/{month}` mkdir_p 必失败
- 真实 client 适配器（`*_real.rs`）整模块标 `#![cfg_attr(coverage_nightly, coverage(off))]`（需真实服务/设备无法 CI 触发）；调度逻辑（`build_target` / `map_*_error` / `tidy_with` 各分支）走 fake 注入 100% 覆盖
- 每方法测三类：OK / client Err / 非自家 scheme 返 `InvalidInput`
- **"未启用 feature 返 Unsupported" 集成测试 MUST `#[cfg(not(feature = "<scheme>-backend"))]` gate**，否则启用 feature 跑 nextest 会 fail
- 远端 op 失败的结构化日志统一在 `remote.rs::map_and_log`（非泛型单点，避免 monomorphization 覆盖率重复计数；9 个 `map_err` 位点全接入）；新增远端日志沿用此单点，**勿**在 smb/adb/mtp 各自加

## 配置与日志
- 运行时配置：`config.yaml`（项目根）+ `src/usecases/config.rs`，`config()` 返 `&'static Config`（`OnceLock`）
- 切换配置：`TIDYMEDIA_CONFIG=/path/to.yaml`；语法 `${VAR:-default}` 由 `expand_env` 自实现（不引 dotenv）
- `expand_env` 嵌套 `{}` 默认值按括号配对解析；展开后以 `{` 开头的值（如 `archive_template`）在 yaml 中 MUST 加引号，否则被 YAML 当 flow mapping 致整个配置回退默认
- 运行时配置非法值校验走 `frameworks/config.rs::sanitize`（warn + 回退默认，与 parse 失败回退 `Config::default` 同哲学；已覆盖 `unique_name_max_attempts==0`、非法 `archive_template`、非法 `log.level`）；新增可校验字段在此加分支，勿在消费点散写
- 结构化日志字段约定：`feature` / `operation` / `result`（CLI 工具无 request_id/user_id）
- `UtcOffset::from_whole_seconds` 范围 ±25:59:59，越界返 `None`，用 `.unwrap_or(UtcOffset::UTC)` 兜底
- **R1 外置**：`copy.{timezone_offset_hours, unique_name_max_attempts, archive_template}` / `exif.valid_date_time_secs` / `backend.smb.{default_user,workgroup}` / `backend.adb.{server_host,server_port}` / `log.level`（CLI `--log-level` 缺省值，优先级 `RUST_LOG` > flag > 配置）。曾有的 `*.timeout_secs` 与 `backend.mtp.*` 因无消费点已删（依赖库无 timeout API、MTP 是 stub）——库/实现就绪时随消费链一起加回，勿先加配置占位
- **不外置的合理例外**：spec §X 算法常量（`EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS` / `MTIME_VS_P0_HINT_SECS`）/ 协议字面量（`IMG_` / `DSC_` / `Screenshot_` / `XMP_KEY`）/ 日志维度名（`FEATURE_*`）/ lookup 表（`MONTH`）/ 流式哈希（`FAST_READ_SIZE`、`STREAM_CHUNK = 1 MiB`、`MIME_SNIFF_BYTES = 256`）
- `src/usecases/copy/ops.rs` 的 `println!("\"{}\"\t\"{}\"", src, dst)` 是 CLI 脚本可读输出（dry-run + 完成回执），**不是** R3 日志路径，不要改成 tracing

## Android / 移动端（feature `android-app`）
- uniffi 0.31 proc-macro 模式：lib.rs 顶层 `uniffi::setup_scaffolding!()` + `#[uniffi::export]` / `#[derive(uniffi::Record)]` / `#[derive(uniffi::Error)]` 注解；不需要 build.rs / .udl
- **`#[derive(uniffi::Error)]` 字段名不能叫 `message`**：uniffi 生成 `class Generic(val message: String)` 与 `kotlin.Exception.message` 撞名编译失败；用 `text` / `detail`。同类 Throwable getter 名（`cause` 等）也要避开
- `[lib] crate-type = ["rlib"]`：rlib 让桌面集成测试链得上；cdylib（给 Android JVM dlopen）**不写死在 Cargo.toml**——Windows 上会与 bin 的 tidymedia.pdb 同名冲突（cargo#6313），交叉编译时用 `cargo rustc --crate-type cdylib` 按需覆盖；不要换 staticlib
- 交叉编译：`cargo ndk -t aarch64-linux-android -p 30 --output-dir mobile/android/app/src/main/jniLibs rustc --lib --crate-type cdylib --release --features android-app`
- Kotlin 绑定：`uniffi-bindgen generate --library <libtidymedia.so> --language kotlin --out-dir <dir>`。**`uniffi --features cli` 装出的 binary 实际叫 `uniffi-bindgen`**（不是 `uniffi-bindgen-cli`）
- `src/frameworks/mobile.rs` 无独立 use case：直接 `tidy_with(&DefaultBackendFactory, Commands::Copy {..})` 复用 CLI 路径（YAGNI）；feature off 时 cfg 排除
- **`tidy_with` 单一入口返 `CommandResult` enum**（`Copy(CopyReport)` / `Find(FindReport)`）：CLI 走 `tidy(..).map(|_|())` 丢弃，mobile/Android 直接 `match` 取 report；**MUST NOT** 新增 `*_report()` 专用包装函数复制 dispatch 逻辑
- **mobile FFI 嵌套集合 MUST `Vec<Record>`**（如 `Vec<MobileDuplicateGroup>`），禁 `paths.join(",")` CSV——路径含逗号会被 Kotlin `split(",")` 拆错段；uniffi 0.31 原生支持嵌套 Record sequence
- 实测工具链：JDK 25 (Temurin) + Gradle 9.1 + AGP 8.10 + Kotlin 2.0.21 + NDK r26d + SDK android-35（AGP 8.7 不支持 JDK 25）。`ANDROID_HOME ≠ ANDROID_NDK_HOME`：cargo-ndk 只读 NDK，Gradle build 还需 SDK，两个环境变量都要设

## 项目 Gotcha
- **测试 Windows 可移植**：Unix-only API（`PermissionsExt`/`OsStringExt`/`UnixListener`/chmod 类）测试 MUST `#[cfg(unix)]` gate、import 放函数内；测试中路径前缀比较 MUST 用 `file_info::full_path` 规范化（绝对路径透传），MUST NOT 手动 `canonicalize()`——用户目录为符号链接时（`C:\Users\x` → `D:\Users\x`）canonicalize 解析出不同盘符，与索引存储不匹配（file_index_tests 已踩坑）；find 脚本输出断言用 `cfg!(target_os = "windows")` 选 `DEL `/`:` vs `rm `/`#`
- Cargo.toml 多数 dep 用 `"*"` 通配；`cargo update` 可能拉到不兼容主版本（sha2 0.10→0.11 已踩坑），主版本升级前先 dry-run
- **本地 `[patch.crates-io]` 验证依赖 patch**：`cp -r ~/.cargo/registry/src/<idx>/<crate>-<ver>/ <vendor>` + patch + `[patch.crates-io] <crate> = { path = "..." }` → `cargo build` 触发重编。**Cargo.lock 中 patched crate 无 `source =`/`checksum =`** 即生效证据。独立编 patched crate 可能挂 build script（MSVC 链接配置缺失），通过 [patch] 在主项目内编 OK；验证通过后切 git URL 留 PR
- **远端 `RemoteClient::read` 整文件入堆**（已知限制，trait 文档有 WHY）：pavao `SmbFile` 借用 client 生命周期装不进 `Box<dyn MediaReader + 'static>`、`adb_client` pull 是回调式写入 API——真正流式需换库或线程+管道，YAGNI 暂不做；大视频在内存受限环境（Android）有 OOM 风险
- **远端 `mkdir_p` 是真递归**（`remote.rs::mkdir_recursive`）：自底向上 stat 找存在的祖先再逐层 mkdir（`AlreadyExists` 容忍、stat 非 NotFound 直接传播）。远端协议 mkdir 多为 POSIX 单层语义（pavao 父层缺失返 ENOENT）；`FakeRemoteClient::mkdir` 不校验父目录，单层 vs 递归的差异 fake 测不出来，须用「中间层注入错误/断言中间层 metadata」类测试钉行为
- **存在性查询的 Err MUST 传播，MUST NOT `unwrap_or(false)`**：把"查询失败"吞成"不存在"会让后续 open_write truncate 覆盖已有文件、move 模式连源一起删（naming.rs 已踩坑修复）；保守兜底方向是 `true`（多重试）而非 `false`（敢覆盖）
- **测试 shim 必须 `#[cfg(test)]` gate**：`Info::from` / `Index::visit_dir` / `Exif::from_path_with_offset` / `adapters/backend/fake_remote` 是包 backend-aware API 的旧入口，仅测试用；未 gate 会让 release build 报 `dead_code`
- **`#[cfg(test)]` 标在方法/import 上，不要标在 `impl Foo {}` 块上**：同块生产方法会被一起 gate 掉。`cargo build --release` 与 `cargo build --tests` 两边都要跑
- **`--all-features` clippy 与 `#[cfg(not(feature))]` test 联动**：启用全部 feature 后，gate 掉的 test fn 对应 imports 必须用同样 `#[cfg(not(all(feature = "smb-backend", feature = "mtp-backend", feature = "adb-backend")))]` 包裹，否则 `unused_import` error
- **clippy 1.95 `doc_markdown` 扩大**：含点号的文件名、含下划线的标识符在 `///` 或 `//!` 注释中均需反引号包，否则 `--all-features` 下 `-D warnings` 报 error
- **`chrono::TimeDelta::seconds(i64)` 会 panic**（secs > ≈ `i64::MAX/1000`），`?` / `.ok()?` 截不住——外部 timestamp（Takeout JSON / 用户输入）解析 MUST 用 `try_seconds()?` + `DateTime::checked_add_signed`
- **重复组容器 MUST `Vec<DuplicateGroup { size, paths }>`**，MUST NOT `BTreeMap<size, _>`：size 作唯一键会让同 size 不同 content 的两组互相覆盖（`file_index.rs::filter_and_sort`）
- **路径前缀匹配 MUST 校验分隔符边界**：`"/photos_backup/x".starts_with("/photos")` 会误判为 output 内须保留——见 `find.rs::under_prefix` 已封装。**prefix 末尾 `/`/`\\` 在 helper 内剥**：dry-run 下 canonical_prefix 保留尾斜杠时朴素 `starts_with` 会让 `rest='img.jpg'` 既非空也不以分隔符开头而误判为不在前缀下
- **含 secret 字段的 struct MUST 手写 `Debug` 而非 derive**：`SmbTarget.password` 经 `derive(Debug)` 会让 `debug!(target=?t)` 把明文写进日志（P0 §12 违反）；手写 Debug 把 secret 字段渲染成 `Option<&"***">` 占位即可（参考 `adapters/backend/smb.rs::SmbTarget`）
- **外部 JSON schema 同一字段历史上可能 String 或 Number**：Google Takeout `photoTakenTime.timestamp` 多版本类型不同，单字段严格 `String` 会让 P3 候选静默丢失；用 `serde_json::Value` + 二态 match 比 `deserialize_with` 简洁（参考 `adapters/sidecar.rs::parse_takeout_json`）
- **`ReportSink` trait 用 `enum Report<'a>` + 单方法 `write(&Report<'_>)`** 收敛多报告类型；对象安全 + 新增 report 变体不强制升级既有 impl（替代旧 `write_copy` / `write_find` 双方法 boilerplate）
- **同盘 move fast-path**：`do_copy` 在 `opts.remove && src.backend().scheme() == "local" && output_backend.scheme() == "local"` 时走 `Backend::rename`（`LocalBackend::rename` 内部 `fs::rename` 同卷原子 + `CrossesDevices` fallback 到 `fs::copy + remove`）。**MUST NOT** 在 Rust 层用 `metadata().dev()` / `GetVolumeInformationByHandleW` 重复判同卷——OS 内核是 same-volume 判定唯一权威源，能识别 subst/junction/mount point/bind mount/btrfs subvol 等所有边界，自己再判必漏
- **dry-run log / 路径聚合分析用 Python（heredoc 或 `uv run xxx.py`）**：tidy-verify Step 3/4 已从 awk 迁到 `.claude/scripts/tidy-verify/*.py`（避 awk `\` regex 在 bash 单引号下二次解析致 `unterminated regexp`）；临时一次性分析用 `cat > /tmp/tm/x.py <<'PYEOF' ... PYEOF` + `cd /tmp/tm && uv run x.py`（uv 在 Windows 解析 `/tmp/...` 与 bash 不一致 → 相对路径绕开）。**Windows 输出陷阱**：Python stdout 默认 CRLF 与下游 grep/diff LF 口径不一致 MUST `sys.stdout.reconfigure(newline='\n')`；终端 stdout 默认 GBK 把 UTF-8 中文显示成 `��` 乱码 → 含中文/路径的报告 MUST 写 UTF-8 文件再 `cat` 读；Write 工具落盘含 `'\\'` 的 Python 字符串字面量会吞反斜杠（用 `SEP = chr(92)` 或 heredoc 绕开）；Python `re` alternation 是 leftmost-first 不是 POSIX leftmost-longest，迁 awk regex MUST 把长 token 放前（`(1[012]|0?[1-9])` 而非 `(0?[1-9]|1[012])`，否则 `2008-10` 抢吃成 `2008-1`）
