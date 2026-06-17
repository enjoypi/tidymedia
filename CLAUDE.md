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
- **`tract-onnx` + `image` 默认编译**（旧 `ocr-detect` feature 已废）：`move-text-shot` + `cull` 子命令共用 tract-onnx 推理 + image 解码栈；ONNX 模型**走 git-lfs 存放 `models/`**（4 个 face + 1 个 OCR，合计约 35 MB；clone 后 `git lfs pull` 或先期 `git lfs install` 自动拉取），`scripts/download_models.sh` + `simplify_onnx.py` 为初次装配脚本（用户 onnxsim 化简 + 固化静态 input shape）：
  - `move-text-shot`：`backend.ocr.det_model_path` / `TIDYMEDIA_OCR_DET_MODEL`（`PaddleOCR DBNet det.onnx`，约 5–10 MB；用户自备）
  - `cull`：`backend.face.{scrfd,facenet,facemesh,eyestate}_model_path` 4 个路径，已落地默认值：
    - SCRFD-10G (`models/scrfd_10g_bnkps.onnx`，antelopev2；500M 变体无开源 onnx)
    - MobileFaceNet (`models/mobilefacenet.onnx`，128 维 embedding；foamliu/MobileFaceNet pt→onnx，**非官方 512 维**)
    - MediaPipe FaceMesh (`models/face_mesh_192x192.onnx`，PINTO_model_zoo 静态版)
    - YOLOv8 EyeState (`models/eyestate_yolov8.onnx`，MichalMlodawski/open-closed-eye-detection；**非 MobileNetV3 softmax**，decode 取 `[1, 6, 8400]` anchor max closed conf)
  - 任一路径为空时调用立即返 `InvalidInput`（懒加载，构造时不读文件；首次 `has_text` / `detect_faces` 触发 `load_runnable`）
  - **MobileFaceNet 128 维而非 512 维**：foamliu 训练规格；`FaceEmbedder` trait 签名 `[f32; 128]`，切换 512 维变体需同步改 `EMBED_DIM` 与 trait 接口
  - **YOLOv8 EyeState decode 与现有 MobileNetV3 假设不同**：输入 640×640 letterbox 而非 64×64 直接 resize，输出检测头 max conf 而非 softmax；closed_index=1 按 README 描述（open=0 / closed=1）
- `bin/exiftool/exiftool.exe`（仅开发调试）：常用 `-time:all -s <f>` 查时间字段、`-P '-AllDates<Filename' -overwrite_original <dir>` 按文件名批量改时间保 mtime；单文件按值写 `-P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=..."`
  - **中文路径**：portable exiftool 缺 `Win32API::File`，命令行 UTF-8 直传 0 文件；要 UTF-8 路径走 **tempfile + `-@ <file>` + `-charset filename=UTF8`**（小写 `filename`）；省掉 `-charset` 走 ANSI 系统调用也稳（stdout 乱码不影响 EXIF 抽取）
  - 写操作默认产 `<file>_original` 备份，MUST 清理（magic-bytes 仍 JPEG 会被 tidymedia 当媒体归档）
  - **`-T` Windows 末字段带 `\r`**：CRLF 让 awk `$NF` 拼成 `value\r`，终端 `\r` 盖行首致 print 错乱、`$NF=="value"` 永假；awk 入口 MUST `sub(/\r$/, "", $NF)`。**项目内已统一迁 Python**（见末节「dry-run log / 路径聚合用 Python」），此坑仅在临时手写 awk 调试时遇到
- nom-exif 内部 `tracing::info!/warn!` 大量输出，`install_logging` 用 EnvFilter 默认压 `nom_exif=error`，保留 `RUST_LOG` 覆盖；trace 调试 `RUST_LOG=nom_exif=trace`，span 名是 `#[tracing::instrument]` 函数名嵌套
- nom-exif 3.5 把 MKV `DateUTC` 合并到 `TrackInfoTag::CreateDate`（无独立 tag）；MP4/MOV vs MKV/WebM 走 MIME 嗅探（`video/x-matroska` / `video/webm`）分流 `Source::MkvDateUtc` vs `QuickTimeCreateDate`
- nom-exif `Exif::get(tag)` 仅读 IFD0/MAIN；GPS 子 IFD 标签必须 `Exif::iter()` 按 tag code 匹配
- **老 QuickTime `pnot`/`mdat` 首 box MOV**（NIKON COOLPIX S5/P5000、Casio 等 2003-2008 早期机型，及 `moov` 在文件末尾的 mdat-first 变体）nom-exif 3.6.0 `parse_bmff_mime` 拒识。fork patch 在入口短路返 QuickTime mime（**必须 `BoxHeader::parse` 仅解析 header**——`BoxHolder::parse` 要求整 body 入 buffer，数 MB mdat 必 Incomplete 让短路静默失效）：`Cargo.toml [patch.crates-io] nom-exif = { git = "https://github.com/enjoypi/nom-exif.git", branch = "feat/legacy-quicktime-pnot" }`；上游 PR 合并后切回 `*` 删 patch
- **AVI（RIFF）nom-exif 不支持**：老相机（Fujifilm FinePix 等）拍摄时间在 `LIST hdrl > LIST strl > strd` chunk（`AVIF` 魔数 + 裸 TIFF IFD，offset 基准 = strd+8），`entities/riff.rs` 自解析填 `Exif` 的 DTO/CreateDate/ModifyDate/make/model；`entities/exif/types.rs::from_reader` 按 `video/x-msvideo` 先于泛 video 分流。**IFD 字节解析抽 `entities/tiff_ifd.rs` 公共模块**（支持 II/MM byte order + IFD0 + ExifIFD 子 IFD），与 PNG eXIf chunk / JPEG APP1 fallback 共享。**双入口**：`parse_tiff(payload)` 接完整 TIFF header（II/MM + magic + IFD0 offset，PNG/JPEG 路径），`parse_ifds(base, ifd0_off, order)` 接裸 IFD（无 header，RIFF strd 路径，offset 不含 header 偏移）
- **`tiff_ifd::scan_ifd` 是 lenient**：中段 entry 越界用 `break` 而非 `?`，已写入 `out` 的前 K 条字段保留；调用方 `parse_ifds` 整体返 `Some(部分填充)`。截断 fixture（PNG eXIf chunk 声称 N entries 但 buffer 只够 K 条）测试预期 `Some(...)` + 部分字段非空，**不**断言 `None`；只有 `count` u16 本身读不到才返 `None`
- **PNG `eXIf` chunk nom-exif 不解析**：PNG 1.5+ 自定义 chunk `eXIf` = 完整 TIFF/EXIF header（II/MM + 0x002A magic + IFD0 + ExifIFD），Lightroom/相机直出 PNG 常含。`entities/png.rs` 自解析（chunk header BE u32 长度 + 4 字节 type，不验证 CRC YAGNI），`entities/exif/image_png.rs::populate_png_dates` 调 `tiff_ifd::parse_tiff` 抽 DTO/CreateDate/ModifyDate/Make/Model；`types.rs::from_reader` 按 `image/png` 先于泛 `image/` 分流。eXIf 缺失或 TIFF 损坏退回 XMP fallback（Lightroom 常并行写 PNG eXIf + XMP）；fixture `tests/data/sample-png-exif.png`（`tests/fixtures/gen_png_exif.py` 生成）
- **JPEG nom-exif `parse_exif` 整体失败 fallback**：Canon EOS 7D 等 MakerNotes 偏移异常机型（exiftool 报 `Adjusted MakerNotes base by -126`）让 nom-exif 抛 Err 致 DTO/CreateDate/Make/Model 全丢。`entities/exif/image_jpeg.rs::parse_jpeg_app1_exif` 扫 JPEG APP1 段（`FF E1` marker + BE u16 长度 + `Exif\0\0` magic + 完整 TIFF header）调 `tiff_ifd::parse_tiff` 抽核心字段；`populate_image_dates` 的 nom-exif Err 分支前置 fallback，三态闭合（fallback 成功 → 直填 / fallback Err → 退 XMP fallback / 主路径 Ok 但双 0 → XMP fallback）。fixture `tests/data/sample-jpeg-app1-broken.jpg`（合成，模拟 ExifIFD 越界 count；累积真实 7D 样本后替换）。**fallback fixture 合成套路**：`ExifIFDPointer` → 子 IFD 声称 `count=10000` 但实际只放 1 entry → nom-exif 扫越界整体拒绝，fallback 看 IFD0（Make/Model）+ 子 IFD 第一 entry（DTO）仍能读出。集成测试 MUST 配「nom-exif 主路径真失败」反向断言（`parse_exif` Err 或返空），防 fixture 失效后 fallback 路径无意识地绕过
- **M2TS/AVCHD nom-exif 不支持**：Canon 摄像机时间在 H.264 SEI `user_data_unregistered`（MDPM UUID），`entities/m2ts.rs` 按 AVCHD video PID `0x1011` 重组 192B BDAV payload 直搜 UUID（不解析 PMT/NAL，YAGNI），EP 字节 `00 00 03` 按 RBSP 剥离。tag `0x18`=[时区,世纪,年,月] BCD、`0x19`=[日,时,分,秒] BCD；时区字节忽略走 naive+配置时区；仅填 DTO（单一拍摄时刻不伪造 P1）；fixture `tests/data/sample-canon-avchd.m2ts`
- **H.264 RBSP emulation-prevention strip 是无条件的**：`m2ts.rs::strip_emulation_prevention` 在 `00 00` 后 strip ANY `0x03` 是 H.264 spec 要求（编码器保证 RBSP 不含 `00 00 0[0-3]`，所以任何 `00 00 03` 必是 EP 字节，无需判 next byte）。code-review 易误判为"应判 next byte"，实际是规范行为
- **JPEG/HEIC/TIFF XMP fallback**：Lightroom/Bridge/ACR re-tag 后 EXIF IFD0 可能仅剩 `ModifyDate`，原始时间在 XMP `photoshop:DateCreated`（→ DTO/P0）与 `xmp:CreateDate`（→ CreateDate/P1）。`populate_image_dates` 双 0 时扫已 buffer 头 64 KB（`XMP_SCAN_BYTES`），`entities/xmp.rs` 抽 attribute 形式（element/ExtendedXMP YAGNI），`adapters/sidecar.rs::parse_xmp_date` 复用同实现；fixture 用 `-api Compact="Shorthand,NoPadding"` 才出 attribute 形式
  - **seek 失败仍跑 XMP fallback**：head 已读入不浪费，否则非可寻址 reader 的 re-tag 图退回 mtime 归错桶
  - **XMP 未闭合 `<!--` 按 strict 抹至 EOF**：lenient 会把注释中 `photoshop:DateCreated="OLD"` 误读为合法属性

## 测试与覆盖率（项目特有；通用套路见 rust-p1 §5）
- **覆盖率门槛 region/function/line/branch 四项 MUST 全 100%**（Linux + `--all-features` + ignore-regex 严格口径 + `--branch`）。**Windows 默认 feature 口径可能有平台差异 miss**（`uri.rs`/`sidecar.rs`/`filename.rs`/`resolve.rs`，多为 feature-gated + multi-binary instance）：Windows 本机判定 = 新增代码 100% + TOTAL miss 与 main 持平；怀疑回归先 `git stash` 对照
- 严格 100%：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch --ignore-filename-regex='(_real\.rs|adapters/(ocr|face)/tract_[a-z]+\.rs|usecases/cull/run\.rs)$' --all-features`；`lib.rs` / `bin/tidymedia.rs` 顶 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]`；`Cargo.toml [lints.rust] unexpected_cfgs` 注册 `cfg(coverage_nightly)`。ignore-regex 三类 subprocess multi-instance phantom：①`adapters/ocr/tract_dbnet.rs`（OCR 适配器，subprocess `tidymedia` bin 链接但不调 OCR）②`adapters/face/tract_{eyestate,facemesh,mobilefacenet,scrfd}.rs`（4 face detector tract impl，同 OCR phantom + `&&` 短路 4 sub-branch 1 unreachable）③`usecases/cull/run.rs`（cull pub fn 主流程 subprocess 跑过 scan + group + write_group，但 `pick_best` 内 `if score > best_score` BR 在 subprocess 永 0-hit——SCRFD `load_runnable` Err 早 `record_failure + continue`，永不到分数比较；lib unit instance 已真实覆盖此分支）。**判定原则**：summary 因 LLVM 多 instance 累加可能虚报 region/line miss，真值以 lcov `DA`/`BRDA` 无 `,0$` 为准（参考本节「multi-binary instance 陷阱」）
- **`*_real.rs` 走 `--ignore-filename-regex` 而非 `coverage(off)`**：真实 client 适配器调真实 SMB/adb-server CI 不可触发，但 `coverage(off)` 注解侵入源码且 mutation testing 会扫到；命令行 `--ignore-filename-regex='_real\.rs$'` 排除整文件，源码零 attribute。`factory.rs` 的 cfg-gated 真实装配（`build_smb_backend` 等）抽到 `adapters/backend/factory_real.rs` 符合命名约定，由同 regex 排除；Unsupported feature-off 分支保留在 `factory.rs` 由 `tidy_rejects_*` e2e 100% 覆盖
- **`--branch` multi-binary instance 陷阱**：lib unit test binary + `tests/lib_tidy.rs` 集成 binary **共享同一 lib rlib codegen**（crate hash 同，如 `Cs7NY9BuJphfg`），lcov 在该 instance 累加两 binary 的 counter——lib unit 已覆盖的 region 自动让 lib_tidy 也算 covered（**不需 lib_tidy 重复触发**）。真正独立 codegen 的是 `tidymedia` bin（subprocess 通过 `CARGO_BIN_EXE_tidymedia` 启动，cli_smoke / run_cli_flags 入口），它有独立 crate hash（如 `Cs1cCdkLBzXDt`）；subprocess 跑的是 spawn 真实 binary，**无法调 lib internal helpers**——`__<name>` re-export 套路对 subprocess instance 无效。subprocess 跑 find/help/version 时不可达的业务路径（如 `do_copy` 内 stream + remove fail）必须 `coverage(off)` 整 fn 豁免，并把不可达片段抽成 `*_helper` fn 独立 `coverage(off)`（参考 `usecases/copy/ops.rs::remove_src_after_stream_copy`）。其他 instance 计数细节套路：①重构成 `?`（算 region 不算 branch）；②显式 `match` 替 `.map_or_else(closure, closure)` 让 LLVM 跨 instance 合并 region；③用 cargo-llvm-cov JSON 报告（`--json`）查 `instance hash + FN/region count` 定位真实不可达 instance，**不要靠 lcov BRDA**（lcov 是累加后输出，instance 级 miss 不显示）
- **`nm` 反查 instance hash 归属**：lcov 显示某 `Cs<hash>` instance 0-hit 时，`nm target/llvm-cov-target/release/deps/<binary> 2>/dev/null | rg Cs<hash>` 反查该 hash 属于哪个 binary。规律：所有 `*_test`/集成 binary 共享同一 lib rlib codegen hash；`tidymedia-<long>`（无 `.d`、19MB 大体积）是 subprocess 用真 bin，独立 hash
- **tracing macro micro-region 在 release 不订阅 debug 级别时 0-hit**：`debug!`/`warn!` 宏展开生成 closure-form micro-region，release default subscriber 不订阅 debug → closure 不执行；lib unit 测试无 subscriber，subprocess instance 也无 → 这些 micro-region 在 multi-instance 累加下仍 0-hit。含多个 `debug!`/`warn!` 的业务 fn 整体 `coverage(off)` 是合理 workaround，配套抽业务级 helper 用独立 `coverage(off)` 让 wrap/边缘逻辑独立可读（参考 `usecases/copy/ops.rs::do_copy` + `remove_src_after_stream_copy`）
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
- **同 `&&` 表达式在两业务调用点 → 抽 helper 收敛 BR**：相同 `if dto==0 && create_date==0` 出现在主路径 + APP1 fallback 两处 → LLVM 在两 callsite 各生成 5 个 sub-branch BR，需独立测试输入覆盖所有；抽 `fn populate_image_xmp_fallback_if_empty(...)` 让两处共用单点 BR，主路径既有 fixture（`sample-no-dates` / `sample-only-createdate` / `sample-with-exif`）即覆盖 fallback 同一 helper（参考 `entities/exif/image.rs`）。同样适用 `if protect` 嵌套 `&&` short-circuit chain → 重写成 `let (protect, is_survivor) = match { .. }` 让分支决策落 match arm（参考 `usecases/find.rs::render_script`）
- **`Cursor` seek 永不失败**：测 seek Err 传播需手写 FailSeek reader（包 Cursor、seek 恒 Err，blanket impl 自动满足 `MediaReader`）。**MUST `#[derive(Debug)]`**——`MediaReader` blanket impl `Read + Seek + Send + Debug + ?Sized`，缺 Debug 让 `Box<dyn MediaReader>` cast 编译失败
- **code-review fix 流程**：build 干净 + clippy 干净 + nextest 通过 ≠ 完工，MUST 再跑严格 100% 命令（见本节首条）确认四项 100%。新加的 `?` Err arm / `&&` 短路新增 sub-branch / defensive 兜底 arm 都易出 region miss，靠后续单测（喂边界值如 `u64::MAX` / `i64::MAX` 触发 `try_from` Err arm）或抽 helper 收敛
- **mutation testing**：配置在 `.cargo/mutants.toml`（nextest + release + `--all-features`，排除 `*_real.rs`/测试基建；豁免在 `exclude_re` 注明 WHY）。**Windows 本机跑不了 `--all-features`**（smb 仅 Linux）：增量验证临时注释 `additional_cargo_args` 按默认 features 跑，feature-gated mutants 留 Linux。日常 `cargo mutants --in-diff <(git diff main)`；全量审计 ~677 mutants/~3h（2026-06 基线 509 caught / 168 unviable / 0 missed）；定点 `cargo mutants -F '<regex>'`（`-E` 是 exclude）。套路：①计数断言精确 `==`（`>= 1` 杀不掉 `+=`→`-=` usize release wrap MAX）；②与被调函数重复的入参 guard 等价变异，删冗余 guard 单点校验；③`cfg(windows)`/`cfg(not(feature))` 代码单口径必假幸存 → exclude_re；④tracing 字段值抽纯 fn 直测不靠捕日志；⑤防御性不可达 arm 喂错配变体直测；⑥写 kill 测试前确认变异行可达（如 `create_time` 无 EXIF let-else 早返回到不了被变异行）；⑦纯移动重构的 `--in-diff` 是假增量，可缩小到真改逻辑文件；⑧无返回值 tracing 副作用 fn `replace X with ()` 等价幸存 → exclude_re
- **`FakeBackend.copy_file` inject 陷阱**：`check_error` 按 **src loc** 匹配，inject `Op::CopyFile` Err MUST 用 src loc 不是 dst；且 `copy_file` 是 in-memory state 操作不支持跨 fake instance（按 self.state 找 src bytes），cull/group_writer 的 `output_backend.copy_file(src,dst)` 跨 scheme 测试只能①同一 fake + scheme!="local" 走 cross-scheme 分支②或走 stream_copy（参考 `move_text_shot` 的 `TwoSchemeFactory`）
- **`unique_name_*` 耗尽测试用 FakeBackend.add_file 填满 N+1 候选**（base + `_1..=_N`）替手写 `AlwaysExists` trait wrapper：wrapper 要 impl 10+ Backend 方法、每个代理 fn 成新 0-hit region（仅 `exists` 被调）；`add_file` 套路无样板代码（参考 `cull/group_writer.rs::unique_name_in_dir_errors_when_exhausted`）

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
- **`unique_name_max_attempts` 是 N+1 候选**：`naming.rs` 循环 `0..=N`（原名 + _1..=_N），N=10 时实际 11 slot 才耗尽。测试 `fill_collisions` 配套 `1..=10`（不是 `1..10`）
- **`find::render_script` SURVIVOR**：output_prefix=Some 且组内无 path 在 prefix 下 → 首份注释保护并加 `# SURVIVOR (no copy under output)` 标记，其余 active `os.remove`；防用户跑脚本删光全部副本。测试 MUST 允许"首份注释"，不能盲目 `!out.contains("# os.remove(\"<未保护前缀>")`
- **新增 CLI flag** → `adapters/dispatch.rs` 调度透传 + **每子命令路径（copy/move/find）独立 e2e 触发 Some/None 两边**，否则 LLVM branch miss；e2e MUST 含 `run_cli(["tidymedia", ...])` 字符串形式（clap flag 名映射只有该形式能验证）
- **新增 `media_time` 候选 / 调整 P0–P4** → `entities/media_time/priority.rs::Source`/`Priority` → 对应解析模块 → `resolve`/`decision` 裁决 → 补 fixture
- **新增 archive_template 占位符** → `usecases/archive_template.rs::render` 替换分支 + 同文件 `PLACEHOLDERS` 常量（当前 6 项）+ `usecases/config.rs::validate_archive_template`（在 `frameworks/config.rs::sanitize` 被调用）三处同步
- **新增容器/格式 EXIF 自解析** → `entities/<container>.rs`（chunk/segment 遍历）+ 调 `entities/tiff_ifd::parse_tiff`（PNG/JPEG header 入口）或 `parse_ifds`（裸 IFD 入口如 AVI strd）+ `entities/exif/image_<container>.rs` 或 fallback 接入点 + `entities/exif/types.rs::from_reader` 分流（image MIME 前置 / 视频 MIME 类似 `video/x-msvideo` 模式）+ 合成 fixture 与 `tests/fixtures/gen_<container>.py` Python 生成脚本入 `tests/fixtures/gen.sh`
- **新增子命令（如 `move-text-shot`/`cull`）** → `adapters/cli.rs::Commands` enum 加 variant + `adapters/dispatch.rs::CommandResult` 加 variant + `tidy()` partial-failure arm + `tidy_with` match arm + `dispatch_<sub>` 函数 + `usecases/<name>/` 目录 + `usecases/report.rs::Report` 加 variant + `adapters/report_sink.rs::FEATURE_<NAME>` 常量 + match arm + `lib.rs` re-export Report 类型 + e2e MUST 含 `run_cli(["tidymedia","<sub>",...])` 字符串形式 + **若主流程含 detector `load_runnable` 失败 → `record_failure + continue` 模式（subprocess 真 bin instance 永不到内层 BR 比较）→ usecase `run.rs` 加入严格 100% ignore-regex（参考 `usecases/cull/run.rs`）**
- **新增 OCR detector / TextDetector trait 方法** → `adapters/ocr/mod.rs::TextDetector` 定义 + `adapters/ocr/fake.rs::FakeTextDetector` 同步支持新方法 + `adapters/ocr/tract_dbnet*.rs` 真实实现（`*_real.rs` 走 `--ignore-filename-regex` 排除，`tract_dbnet.rs` 因含 subprocess instance phantom miss 也整文件排除——见「测试与覆盖率」节）
- **新增 Face detector / `FaceDetector`/`FaceEmbedder`/`FaceMeshDetector`/`EyeStateClassifier` trait 方法** → `adapters/face/mod.rs` 4 trait 定义 + `adapters/face/fake.rs` 4 Fake 同步 + 4 套 `tract_{scrfd,mobilefacenet,facemesh,eyestate}.rs(+_real.rs)` 真实实现（`*_real.rs` 走 `--ignore-filename-regex` 排除；若主体 `tract_*.rs` 出现 subprocess phantom miss 同 `tract_dbnet.rs` 一并加入 ignore-regex）+ `lib.rs` re-export 4 trait + 4 Fake（`#[doc(hidden)]`）
- **新增 `FaceConfig` 字段** → `usecases/config.rs::FaceConfig` + `BackendConfig.face` + `frameworks/config.rs::sanitize_face()` 加校验分支（对齐 `sanitize_ocr` 套路）+ `sanitize()` 末尾调用 + `config.yaml backend.face:` 子段（每字段 `${TIDYMEDIA_FACE_*:-默认}` env 兜底）+ `usecases/config.rs::tests::config_defaults_match_historical_constants` 加新字段默认值断言

## 项目分层（Clean Architecture）
- 四层（自外向内）：`src/frameworks/` → `src/adapters/` → `src/usecases/` → `src/entities/`
- `bin/tidymedia.rs` **只**调 `tidymedia::run_cli(env::args_os())`，零业务；`lib.rs` 仅模块声明 + re-export
- `usecases/` 仅依赖 `entities/`；通过 `pub use crate::frameworks::config::config;` re-export 让 usecases 内部用 `super::config::config`
- `entities/backend/` 是 Gateway 抽象（`trait Backend` + `SmbTarget`/`AdbTarget`/`MtpTarget`）；具体实现在 `adapters/backend/` 通过 re-export 保留原路径；`file_info`/`file_index`/`exif`/`media_time::sidecar` 都 backend-aware（持 `Arc<dyn Backend>`）
- 目录名是 `usecases`（无下划线）
- **`tidy()` partial-failure Err 与 `tidy_with()` 不对称**：`tidy()`（CLI 入口）在 `CopyReport.failed > 0` 时返 Err 让 `$?` 非 0；`tidy_with(factory, ...)` 直接返 `CommandResult` 跳过该检查。覆盖 `dispatch.rs` partial-failure Err arm MUST 用 `tidy(Commands::Copy {..})`，不能用 `tidy_with`（参考 `tests/lib_tidy/move_failure_recovery.rs::tidy_returns_err_when_copy_partial_failure`）

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
- **feature off stub `new()` 用 `remote::unsupported_backend(feature)`**：smb/adb/mtp 三个 backend 的 cfg-off `new()` 共用单点 helper 构造 `io::Error::Unsupported`，文案 `"<feature> not enabled; rebuild with --features <feature>"`。新增 remote backend 用此 pattern 避免 stub 副本散落

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
- **`chrono::TimeDelta::seconds(i64)` 会 panic**（secs > ≈ `i64::MAX/1000`），`?` / `.ok()?` 截不住——外部 timestamp（Takeout JSON / 用户输入）解析 MUST 用 `try_seconds()?` + `DateTime::checked_add_signed`。**`TimeDelta::milliseconds(i64)` 不 panic**（内部 `(secs, nanos<1e9)` 分离存储），任意 i64 ms 输入安全；code-review 易把两者混为一谈误报"13 位 ms 触发 panic"
- **`epoch_to_candidate(u64)` 用 `try_from + try_seconds + checked_add_signed` 三段守护**：u64 > i64::MAX 通过 `cast_signed` 会折回大负值绕过 1904/future filter 让文件落 1969/12 桶；同样防 TimeDelta 上限 + DateTime 年份范围溢出。任一 None 即返 None 让 caller 退到下一优先级候选
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
- **dry-run log / 路径聚合用 Python**（heredoc 或 `uv run xxx.py`）：tidy-verify Step 3/4 已从 awk 迁到 `.claude/scripts/tidy-verify/*.py`（避 awk `\` regex 在 bash 单引号下二次解析致 `unterminated regexp`）；临时一次性分析用 `cat > /tmp/tm/x.py <<'PYEOF' ... PYEOF` + `cd /tmp/tm && uv run x.py`。**项目根外独立脚本（如 `tests/fixtures/gen_*.py` fixture 生成）** MUST `uv run --quiet --no-project <script>` 否则 uv 在项目根检测 pyproject.toml 致行为不一致。**Windows 输出陷阱**：Python stdout 默认 CRLF 与下游 grep/diff LF 不一致 MUST `sys.stdout.reconfigure(newline='\n')`；终端 stdout 默认 GBK 把 UTF-8 中文显示成 `��` → 含中文/路径的报告 MUST 写 UTF-8 文件再读；Write 工具落盘含 `'\\'` 的 Python 字符串字面量会吞反斜杠（用 `SEP = chr(92)` 或 heredoc 绕开）；Python `re` alternation 是 leftmost-first 不是 POSIX leftmost-longest，迁 awk regex MUST 把长 token 放前（`(1[012]|0?[1-9])` 而非 `(0?[1-9]|1[012])`）
- **find 删除脚本统一 Python 输出**：`render_script` 输出 `#!/usr/bin/env python3` shebang + `import os` 头 + `os.remove("...")` 待删 / `# os.remove("...")` 注释保护，按 `DuplicateGroup` size 降序分组。**路径转义**：`s.replace('\\', "\\\\").replace('"', "\\\"")`；含 `\n` 控制字符暂不处理（YAGNI）
- **tract `outputs[i].cast_to::<f32>()` 返 `Cow<Tensor>` 不能直接 `.as_slice()`**：MUST 三步走 `let cow = ...?; let view = cow.view(); let slice = view.as_slice::<f32>().map_err(io::Error::other)?;`。`view().as_slice::<T>()` 返 `Result<&[T], TractError>`（不是 Option，`.ok_or_else` 编译失败）。`tract_dbnet.rs` 用 `.expect(...)` 掩盖了 API 形态——新接 tract 模型 MUST 用 `map_err` 让 Err 可传播；多输出模型（如 SCRFD 9 tensors）每个 tensor 独立 `view()` 持 binding 防 borrowed value dropped
- **clippy `field_reassign_with_default` (pedantic) 强制 struct literal**：测试 helper `let mut c = T::default(); c.field = v; c` MUST 改 `T { field: v, ..T::default() }`；仅覆盖部分字段时 `..T::default()` 合法，**全字段显式给值时禁用** `..Default::default()`（撞 `clippy::needless_update`）
- **f32 if-else 字面量类型推导陷阱**：`let x = if cond { 1.0 } else { 0.0 };` Rust 推 f64 默认；后续 `x.max(other_f32)` 报 `ambiguous numeric type {float}`。MUST 显式标 `let x: f32 = if ...`，或字面量后缀 `1.0_f32 / 0.0_f32`
- **P0 §1「MUST 用 `#[expect]`」仅约束 `clippy::xxx` lints，不约束 rust 内置 lint**（含 `dead_code` / `unused_imports` / `unused_variables`）：占位实现模块（plan 已注明 e2e 真跑后接入）用 module-level `#![allow(dead_code, reason = "占位实现：xxx 接入后启用")]` 合规；用 `#[expect(dead_code)]` 反而触发 `unfulfilled_lint_expectation` 当未来真接入时
- **新增整模块时先做 lib.rs re-export + dispatch 装配再写 algos 主体**：跳过装配链路直接写 algos 会让整个新模块所有 item 触发 `dead_code` warning（lib.rs 未 re-export → 子模块未 used → 链式传播），数十条 warning 噪声压住真实错误。装配链路（trait re-export + Fake re-export + Commands variant + CommandResult variant + dispatch_xxx + Report variant + sink match arm）先打通形成「死端到 lib API」通路，再写算法实现 build 干净
