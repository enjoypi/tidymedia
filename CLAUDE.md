@~/.claude/rust-p0.md

@~/.claude/rust-p1.md

# tidymedia 开发上下文

按「拍摄时间」去重整理照片/视频的多后端 CLI：扫描 sources（local/smb/adb/mtp 可混合）→ SHA-512 去重 → 按拍摄时间归档到 `output/年/月`。核心算法 = 拍摄时间判定（P0–P4，见「核心算法」节）。Clean Architecture 四层 + Android app（feature `android-app`）。代码注释里的 `docs/media-time-detection.md §X` 是历史 spec 引用（文件已删）。

## Quick Start
- **所有 cargo 命令 MUST 带 `--release`**：`profile.release` 已设 `opt-level = 0`，编译速度与 debug 相当，统一 target 目录避免双份产物
- **ONNX 推理 e2e 真跑必切 opt=3**：`profile.release opt-level=0` 让 SCRFD/MobileFaceNet/FaceMesh/EyeState 推理慢到 e2e 不可行（实测 3×256×256 PNG 跑 4 模型 8+ min 未完成）；真跑前临时 `CARGO_PROFILE_RELEASE_OPT_LEVEL=3 cargo run --release -- cull ...`；日常 lib unit + 集成测试用 Fake 注入完整覆盖业务路径
- **tract 加载分流（动态 vs 静态 shape）**：4 face 模型经 `scripts/simplify_onnx.py` 固化静态 shape → `into_optimized().into_runnable()`；**PaddleOCR `DBNet det.onnx` 是动态 H/W 输入** (`[1, 3, ?, ?]`，simplify 脚本未覆盖) → MUST `into_typed().into_runnable()` 跳 `into_optimized`，否则 tract 对 symbolic dim 做不了形状传播首次加载即 Err
- 构建/运行/dry-run：`cargo {build,run} --release [-- copy /src -o /out [--dry-run]]`
- **典型归档工作流**（4 步串联，详节见各章）：
  - ① `cargo run --release -- copy /src -o /out --dry-run --report /tmp/r.json`（生成 dry-run 报告）
  - ② `/tidy-verify /src /out`（dry-run 比对 + exiftool 对账 + 桶 MISMATCH 排查；见「核心算法」§EXIF vs 归档桶对账）
  - ③ `cargo run --release -- copy /src -o /out`（真跑；改 `move` 子命令做原地搬迁，源会被删）
  - ④ `cargo run --release -- find /out`（兜底查重，输出 Python 脚本由人工 review 后执行）
- 测试 `cargo nextest run --release`；覆盖率 `cargo llvm-cov --release nextest --summary-only`（严格 100% 口径见「测试与覆盖率」）
- lint `cargo +nightly fmt && cargo clippy --release --all-targets --all-features --locked -- -D warnings`；**`--all-features` 仅 Linux 可验**（smb-backend 需 libsmbclient）
- **CLI flag 位置**：`--log-level=debug` 是全局 flag 放最前；`--dry-run` 是子命令级 MUST 放 `copy`/`move` 后
- tidymedia debug 走 stderr；`| tail -N` 会截 copy_file 行让 summary `copied=N` 与日志行数对不上 → 重定向文件再 grep
- MSVC 链接固化在 `.cargo/config.toml`（PATH 中 GNU coreutils `link` 会抢 `link.exe`、vcvars64 在精简 shell 不可用）；换 MSVC/SDK 版本需同步该文件
- rustup default-host + 全部工具链 MUST `x86_64-pc-windows-msvc`（gnu 与 MSVC 链接不兼容）；cargo-llvm-cov 安装不带 `--locked`
- **MSVC build cc-rs 找不到 cl.exe/lib.exe**（tract-linalg 等 build-dep 直调 `cc::windows_registry::find`，绕过 `.cargo/config.toml linker`）：VS18/preview 不被 cc-rs 注册表识别——**但 cc-rs 同时读 VC 环境变量**（`VCINSTALLDIR` / `VCToolsInstallDir` / `INCLUDE` / `LIB` / `LIBPATH` / `PATH` 含 MSVC bin），若**父 shell 已完整加载 vcvars64.bat 且环境变量传入 bash 子进程**（实测 Windows Terminal 启动 bash 时透传 VC env，`env | grep VCINSTALL` 可验证），`cargo build --release` 直接成功无需 `.cmd` 包装。仅当 bash 未继承 VC env 时才需写 `.cmd` 脚本 `call "<vs>\VC\Auxiliary\Build\vcvars64.bat"` + 内嵌 cargo 命令 + **直接执行该 .cmd**（非 `cmd //c "..."` 包装，否则 `VCToolsInstallDir` 不传子进程）
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
- **ONNX 模型装配** 走 `scripts/download_models.sh` + `simplify_onnx.py` 单点；新模型陷阱集中在脚本顶部注释：PINTO tarball 双层嵌套、`torch.onnx.export` 需 `dynamo=False`、`onnxsim` 化简动态 shape 必须 `overwrite_input_shapes`、`uv run --with torch --with numpy --with onnxscript` 三件套
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
- **JPEG nom-exif `parse_exif` 整体失败 fallback**：Canon EOS 7D 等 MakerNotes 偏移异常让 nom-exif 抛 Err 致 DTO/CreateDate/Make/Model 全丢。`entities/exif/image_jpeg.rs::parse_jpeg_app1_exif` 扫 APP1 段（`FF E1` + `Exif\0\0` + TIFF header）调 `tiff_ifd::parse_tiff`；`populate_image_dates` 三态闭合：fallback 成功直填 / fallback Err 退 XMP / 主路径 Ok 但双 0 退 XMP。**fixture 合成套路**：ExifIFDPointer 子 IFD 声称 `count=10000` 实只 1 entry 让 nom-exif 整体拒绝、fallback 仍读出 IFD0 + 首 entry。集成测试 MUST 配「nom-exif 主路径真失败」反向断言（`parse_exif` Err 或返空），防 fixture 失效后 fallback 路径无意识绕过
- **M2TS/AVCHD nom-exif 不支持**：Canon 摄像机时间在 H.264 SEI `user_data_unregistered`（MDPM UUID），`entities/m2ts.rs` 按 AVCHD video PID `0x1011` 重组 192B BDAV payload 直搜 UUID（不解析 PMT/NAL，YAGNI），EP 字节 `00 00 03` 按 RBSP 剥离。tag `0x18`=[时区,世纪,年,月] BCD、`0x19`=[日,时,分,秒] BCD；时区字节忽略走 naive+配置时区；仅填 DTO（单一拍摄时刻不伪造 P1）；fixture `tests/data/sample-canon-avchd.m2ts`
- **H.264 RBSP emulation-prevention strip 是无条件的**：`m2ts.rs::strip_emulation_prevention` 在 `00 00` 后 strip ANY `0x03` 是 H.264 spec 要求（编码器保证 RBSP 不含 `00 00 0[0-3]`，所以任何 `00 00 03` 必是 EP 字节，无需判 next byte）。code-review 易误判为"应判 next byte"，实际是规范行为
- **JPEG/HEIC/TIFF XMP fallback**：Lightroom/Bridge/ACR re-tag 后 EXIF IFD0 可能仅剩 `ModifyDate`，原始时间在 XMP `photoshop:DateCreated`（→ DTO/P0）与 `xmp:CreateDate`（→ CreateDate/P1）。`populate_image_dates` 双 0 时扫已 buffer 头 64 KB（`XMP_SCAN_BYTES`），`entities/xmp.rs` 抽 attribute 形式（element/ExtendedXMP YAGNI），`adapters/sidecar.rs::parse_xmp_date` 复用同实现；fixture 用 `-api Compact="Shorthand,NoPadding"` 才出 attribute 形式
  - **seek 失败仍跑 XMP fallback**：head 已读入不浪费，否则非可寻址 reader 的 re-tag 图退回 mtime 归错桶
  - **XMP 未闭合 `<!--` 按 strict 抹至 EOF**：lenient 会把注释中 `photoshop:DateCreated="OLD"` 误读为合法属性

## 测试与覆盖率（项目特有；通用套路见 rust-p1 §5）
- **覆盖率门槛 region/function/line/branch 四项 MUST 全 100%**（Linux + `--all-features` + ignore-regex 严格口径 + `--branch`）。**Windows 默认 feature 口径可能有平台差异 miss**（`uri.rs`/`sidecar.rs`/`filename.rs`/`resolve.rs`，多为 feature-gated + multi-binary instance）：Windows 本机判定 = 新增代码 100% + TOTAL miss 与 main 持平；怀疑回归先 `git stash` 对照
- 严格 100% 命令：
  - `RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch --ignore-filename-regex='(_real\.rs|adapters/(ocr|face)/tract_[a-z]+\.rs)$' --all-features`
  - `lib.rs` / `bin/tidymedia.rs` 顶 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]`；`Cargo.toml [lints.rust] unexpected_cfgs` 注册 `cfg(coverage_nightly)`
- ignore-regex 两类 subprocess multi-instance phantom：
  - ① `adapters/ocr/tract_dbnet.rs`（OCR 适配器，subprocess `tidymedia` bin 链接但不调 OCR）
  - ② `adapters/face/tract_{eyestate,facemesh,mobilefacenet,scrfd}.rs`（4 face detector tract impl，同 OCR phantom + `&&` 短路 4 sub-branch 1 unreachable）
- **subprocess phantom miss 剔除靠四类微测试**（参考 `usecases/cull/run.rs` 现状）：① 私有 helper 直测（`crop_face_bbox` / `crop_eye_around` / `u32_from_f32_clamped`）；② `inject_reader_error` 触发 `read_to_end` Err；③ `landmarks_5pt = [[1.0; 2]; 5]` 触发 `align_face` 矩阵奇异；④ `facenet.with_error` 触发 `embed_face` Err 命中两 let-else continue
- **判定原则**：summary 因 LLVM 多 instance 累加可能虚报 region/line miss，真值以 lcov `DA`/`BRDA` 无 `,0$` 为准（参考本节「multi-binary instance 陷阱」）
- **`*_real.rs` 走 `--ignore-filename-regex` 而非 `coverage(off)`**：真实 client 适配器调真实 SMB/adb-server CI 不可触发，但 `coverage(off)` 注解侵入源码且 mutation testing 会扫到；命令行 `--ignore-filename-regex='_real\.rs$'` 排除整文件，源码零 attribute。`factory.rs` 的 cfg-gated 真实装配（`build_smb_backend` 等）抽到 `adapters/backend/factory_real.rs` 符合命名约定，由同 regex 排除；Unsupported feature-off 分支保留在 `factory.rs` 由 `tidy_rejects_*` e2e 100% 覆盖
- **`--branch` multi-binary instance**：lib unit + `tests/lib_tidy.rs` 共享同一 lib rlib codegen hash（lcov 累加，lib unit 已覆盖即可），`tidymedia` bin 独立 codegen（subprocess 经 `CARGO_BIN_EXE_tidymedia` 启动，无法调 lib internal helpers）。subprocess 跑 find/help/version 时不可达的业务路径整 fn `coverage(off)` 豁免，不可达片段抽 `*_helper` 独立 `coverage(off)`（参考 `usecases/copy/ops.rs::remove_src_after_stream_copy`）。instance miss 定位用 cargo-llvm-cov JSON `--json` + `nm target/llvm-cov-target/release/deps/<bin> | rg Cs<hash>` 反查归属，**勿靠 lcov BRDA**（累加后看不到 instance 级 miss）。计数收敛套路：① 重构 `?`（算 region 不算 branch）；② 显式 `match` 替 `.map_or_else(closure, closure)` 跨 instance 合并 region
- **tracing macro micro-region release subscriber 不订阅 debug 时 0-hit**：`debug!`/`warn!` 宏展开 closure-form micro-region 在 lib unit + subprocess 都无 subscriber → 多 instance 累加下仍 0-hit。含多 `debug!`/`warn!` 的业务 fn 整体 `coverage(off)`，业务逻辑抽 helper 独立 `coverage(off)` 保可读
- **office 解析子模块 phantom region miss 几乎不可避免**：fn 多 `?` / OR 链 / closure / 泛型 monomorphize 在 lib unit + bin subprocess 两 instance 各 codegen 一份，subprocess 只跑 happy 而 lib 跑边界 → region 切片不重合致 summary 算 phantom miss（lcov BRDA 仍 100%）。务实套路：`parse(reader, mime)` 入口 + 复杂业务 fn（`parse_pdf_d_format` / `parse_tz_offset` / `scan_int_after` 泛型 / 多 let-else）都 `coverage(off)`，业务正确性靠 lib unit `*_tests.rs` 全分支断言；不可达 `?` Err arm（`from_utf8(ASCII)` 等）改 `.expect("internal: digits are ASCII")` 助手消除（CLAUDE.md「逻辑不可达的 `?` 死区消除」实操）
- **subprocess office fixture 集中管理**：`tests/lib_tidy/run_cli_flags.rs::OFFICE_FIXTURES: &[&str]` 常量 + 单 `binary_copy_with_office_fixtures_walks_office_paths` test 自动遍历跑 `copy --include-non-media --dry-run`，让 bin instance 命中所有 office 业务路径消除 multi-instance phantom。新增 office 格式仅需把 fixture 文件名加入数组，无需新增 spawn binary fn
- 改 `Cargo.toml` / `coverage` 属性后必 `cargo +nightly llvm-cov clean --workspace`
- `FakeBackend::inject_reader_error`：`open_read` 成功但 reader `read` 立即 Err，覆盖 stream hash / `sniff_mime` 等 `?` Err 分支；**消息文案 = `io::Error::from(kind)` 即 stdlib `ErrorKind` Display**（`TimedOut` → "timed out" / `PermissionDenied` → "permission denied"），**不含 "injected"**——与 `inject_error(loc, Op, kind)` 走 `io::Error::other("injected ...")` 不同；断言文案 MUST 分清两类注入；`add_file_with_times` 构造 `modified=None`（真实 fs 造不出，`create_time` 边界用）；`DummyClient.mkdir_calls` 原子计数杀「best-effort 调用替换成 no-op」类变异
- **私有 fn 微测试补 region miss**：`#[cfg(test)] #[path = "xxx_tests.rs"] mod tests` 套路下 `use super::*;` 可直测私有 helper（如 `crop_face_bbox`/`u32_from_f32_clamped`），命中 `||` 短路各 sub-branch + NaN/负值 clamp 退化路径，比绕 e2e 调用链（要造合规 SCRFD landmarks + facenet/facemesh 假 trait）触发简洁 5×；适合 helper-style fn 的边界值覆盖
- **fast-path 命中反向证据**：`fs::rename`/`fs::copy` 保留 src mtime，`std::io::copy` 不保留；`filetime::set_file_mtime` 钉 src 后 move 断言 dst mtime 不变即命中（参考 `tests/lib_tidy/move_idempotency.rs::move_local_fastpath_dst_mtime_matches_src`）。fast-path 走 `fs::rename` 不读源，chmod 触发失败改 chmod 555 **src 父目录**（rename 需父目录 write）
- **chmod 触发 stat 类 IO 错误**：chmod 0 **文件** stat 仍成功（要 parent dir execute）；chmod 0 **parent dir** 才让 `try_exists`/`metadata` 返 `PermissionDenied`
- **测试 env helper 套路**（P0 §13 SAFETY DRY）：测试模块顶 `#![allow(unsafe_code)]` + 单点 `fn set_env_var(name, value) { unsafe { std::env::set_var(name, value) } }` / `remove_env_var`（参考 `frameworks/config.rs::tests`）。**sed 批量替换陷阱**：把 helper 自身实现也匹配掉致无限递归 → sed 后手动改回 helper 内 unsafe 块
- **平台条件常量优先于 `if cfg!(...)`**：跨平台行尾/路径分隔符 MUST `#[cfg(target_os="windows")] pub(crate) const X: &str = "\r"; #[cfg(not(...))] pub(crate) const X: &str = "";`。`cfg!()` 让 LLVM instrument 两 arm、dead arm 必 miss；双 `#[cfg]` 让 cfg-out 平台代码不编译。**业务逻辑跨平台分裂应消除而非容忍**
- **Windows 同卷边界本地测**（`tests/lib_tidy/windows_same_volume.rs`，`#![cfg(windows)]`）：`subst` + `mklink /J` 构造「不同前缀同卷」均不要 admin；subst 盘字全局共享 → `.config/nextest.toml` 加 `[test-groups.windows-volume-mut] max-threads=1` + filter 强制串行；释放盘字用 RAII Drop guard `subst Y: /D`
- **`--all-features` 覆盖**：`dispatch.rs` 的 `usecases::copy(..)?` Err arm，现有 `tidy_rejects_*` 用 `#[cfg(not(feature="smb-backend"))]` gate，全 feature 跑覆盖率不编译 → 用 output 父路径占成普通文件触发 `mkdir_p` Err 兜底（参考 `dispatch_and_cli.rs::tidy_copy_propagates_mkdir_error_when_output_parent_is_file`）
- **`--all-features` 严格覆盖也须 100%**：feature 启用侧用 `#[cfg(feature="...")]` 镜像 `tidy_rejects_*`（`tests/lib_tidy/real_factory.rs`）。`RealMtpClient::new` 是 stub 必 Err → 确定性触发 `dispatch.rs` 各 `?` Err arm；`PavaoClient::new`/`ADBServerDevice::new` 仅初始化不连网可直接断言。feature 启用时 `factory.rs` 用 `use super::factory_real::build_smb_backend;` 形式替换 dispatch 目标，dead 兜底走 ignore-regex 排除（不用 `coverage(off)`）。`mobile.rs` 不可达防御抽纯 helper（`expect_copy`/`expect_find`/`copy_status`/`stats_from`/`mobile_report_from`）直测；调用点用 `.map` 替代 `?` 消除永不触发的 Err region
- **子行 region miss 定位**（lcov `DA`/`BRDA` 不显示）：`cargo +nightly llvm-cov report --release --text` 复用上次 profdata（**`report` 子命令不接 `--all-features`**，feature 集随上次 run），输出 `^0` 标记即 miss
- **branch miss 精确定位**：`llvm-cov report --release --lcov` 过滤 `BRDA` 第 4 字段 0
- **逻辑不可达的 `?` 死区消除**：循环边界用 `while let Some(x) = buf.get(start..end)` 替手写哨兵（参考 `entities/riff.rs`）；`buf.get(idx..)?` 当 idx 来自 `position()`/`find()` 时改 `&buf[idx..]`（`?` Err arm 不可达，参考 `entities/xmp.rs::find_xmp_packet` / `find_attr_rfc3339`）；纯类型安抚用 `unwrap_or(常量)` 替 `.ok()?`
- **fn pointer 依赖注入测 Err arm**：thin wrapper 在生产路径下 OK arm 必触发但 Err arm 难真触发（如 `walk_entry_to_io` 的 `entry.metadata()` 失败、`rename_or_fallback` 的 `remove` 失败）→ 抽 `*_with(fn pointer)` 参数化版本，生产入口默认传 real fn，测试注 mock 返 Err 命中 `.map_err` 闭包 region（参考 `local.rs::walk_entry_to_io_with` / `rename_or_fallback_with`）
- **私有 fn 加新必填参数 → `#[cfg(test)]` shim + `use … as 原名` alias 让 N 处测试调用零改动**：例 `ops.rs::do_copy` 加 `mkdir_cache: &mut HashSet<Location>` 后，同模块加 `#[cfg(test)] pub(super) fn do_copy_with_default_cache(...) { let mut mc = HashSet::new(); do_copy(..., &mut mc, ...) }`，`mod.rs::#[cfg(test)] use self::ops::do_copy_with_default_cache as do_copy;` 让 `super::super::*` glob import 后 12 处测试调用 `do_copy(` 完全不动；生产路径走原 fn 持 loop 级状态，测试桩每次新建默认值。比起加默认参数（破坏强制决策语义）或成片 sed 测试同步更整洁
- **`DummyCtx::fail_after(n: usize)` 模式**：`from_location` 第 N 次调用之后开始返 Err，让 `walk_recursive` 的子项 `from_location` Err arm 在根 `build_target` 成功的前提下被命中（用 `Arc<AtomicUsize>` 计数；参考 `remote_test_helpers.rs::DummyCtx`）
- **assert! `&&` 拆两条 assert**：`assert!(a && b, ...)` 让 LLVM 把 `&&` 短路为两 sub-branch BR，False 路径（assert 失败）天然 0-hit；拆 `assert!(a); assert!(b);` 单 contains 不分子分支，BR 100%
- **`assert!(cond, "got: {}", expr)` panic 路径子表达式 region miss**：assert 通过时 panic 输出闭包内 `expr` 不求值 → LLVM 算 0-hit region；改 `let val = expr; assert!(cond, "got: {val}");` 提前求值消除（与「assert! `&&` 拆两条」同类，专攻"assert 失败 panic 路径"的 phantom miss）
- **DCT pHash 测试 fixture 设计**：低分辨率 + checker pattern 经 phash 内 32×32 缩放高频损失严重，不适合做缩放/重压缩稳定性测试；用 random/gradient fill + JPEG 质量 90 + ≥ 256×256 大图；JPEG 重压缩断言 `Hamming ≤ 12`、不同分辨率同图 `≤ 20`
- **同 `&&` 表达式在两业务调用点 → 抽 helper 收敛 BR**：相同 `if dto==0 && create_date==0` 出现在主路径 + APP1 fallback 两处 → LLVM 在两 callsite 各生成 5 个 sub-branch BR，需独立测试输入覆盖所有；抽 `fn populate_image_xmp_fallback_if_empty(...)` 让两处共用单点 BR，主路径既有 fixture（`sample-no-dates` / `sample-only-createdate` / `sample-with-exif`）即覆盖 fallback 同一 helper（参考 `entities/exif/image.rs`）。同样适用 `if protect` 嵌套 `&&` short-circuit chain → 重写成 `let (protect, is_survivor) = match { .. }` 让分支决策落 match arm（参考 `usecases/find.rs::render_script`）
- **`Cursor` seek 永不失败**：测 seek Err 传播需手写 FailSeek reader（包 Cursor、seek 恒 Err，blanket impl 自动满足 `MediaReader`）。**MUST `#[derive(Debug)]`**——`MediaReader` blanket impl `Read + Seek + Send + Debug + ?Sized`，缺 Debug 让 `Box<dyn MediaReader>` cast 编译失败
- **code-review fix 流程**：build 干净 + clippy 干净 + nextest 通过 ≠ 完工，MUST 再跑严格 100% 命令（见本节首条）确认四项 100%。新加的 `?` Err arm / `&&` 短路新增 sub-branch / defensive 兜底 arm 都易出 region miss，靠后续单测（喂边界值如 `u64::MAX` / `i64::MAX` 触发 `try_from` Err arm）或抽 helper 收敛
- **mutation testing**：配置 `.cargo/mutants.toml`（nextest + release + `--all-features`，排除 `*_real.rs`/测试基建）。日常 `cargo mutants --in-diff <(git diff main)`；定点 `cargo mutants -F '<regex>'`（`-E` 是 exclude）。Windows 不可跑 `--all-features`（smb 仅 Linux）。核心套路：
  - ① 计数断言精确 `==`（`>= 1` 杀不掉 `+=`→`-=` usize wrap MAX）
  - ② 单口径 cfg（`cfg(windows)`/`cfg(not(feature))`）+ 无返回值 tracing 副作用 fn 必假幸存 → exclude_re 注明 WHY
  - ③ 防御性不可达 arm 喂错配变体直测，tracing 字段值抽纯 fn 直测
  - ④ 算术常量 `*` → `+` 等价变异用中间边界值杀（`1024*1024` 变 `2048` → 用 `5000` 区分原代码触发回退、变异不触发）
- **`FakeBackend.copy_file` inject 陷阱**：`check_error` 按 **src loc** 匹配，inject `Op::CopyFile` Err MUST 用 src loc 不是 dst；且 `copy_file` 是 in-memory state 操作不支持跨 fake instance（按 self.state 找 src bytes），cull/group_writer 的 `output_backend.copy_file(src,dst)` 跨 scheme 测试只能①同一 fake + scheme!="local" 走 cross-scheme 分支②或走 stream_copy（参考 `move_text_shot` 的 `TwoSchemeFactory`）
- **`unique_name_*` 耗尽测试用 FakeBackend.add_file 填满 N+1 候选**（base + `_1..=_N`）替手写 `AlwaysExists` trait wrapper：wrapper 要 impl 10+ Backend 方法、每个代理 fn 成新 0-hit region（仅 `exists` 被调）；`add_file` 套路无样板代码（参考 `cull/group_writer.rs::unique_name_in_dir_errors_when_exhausted`）
- **`config.yaml` 默认值改 → `OnceLock` 一次性加载，依赖默认值的集成测试 MUST 切独立 yaml**：`tests/lib_tidy/cull.rs::write_temp_config` 写临时 yaml + `set_var("TIDYMEDIA_CONFIG", ...)` 让 `frameworks::config::config()` 加载指定路径；空字段（如 `scrfd_model_path=""`）触发 `InvalidInput` 测试同套路——不能再依赖「不切 config 就拿空字符串」
- **`fake_remote::list` 返直属子项**（非 flat 子树）：`is_direct_child(child, parent)` 校验 `child = parent + sep + segment` 且 segment 无内嵌 `/`/`\\`；多层目录 fixture 走 `client.add_dir(path)` + `client.add_file(path)` 组合，让 `walk_recursive` 的 `EntryKind::Dir` 递归分支被实际驱动（旧 flat 语义让该分支永久 dead）；空 parent 是 "list 全部 keys" 测试后门
- **pHash 稳定性测试 brightness shift 用 random noise 而非 gradient**：smooth gradient（如 `(x+y) linear`）DCT 系数集中在低频且中位数附近密集，小幅亮度 ±5 翻转多 bit 让 Hamming 距离断言失败（实测 22）；high-entropy random noise（`(i*37) ^ (i>>3) & 0xff`）DCT 系数远离中位数，brightness shift 翻转极少（Hamming ≤ 8 稳定通过）；CLAUDE.md「random/gradient fill」并列但实际首选 random for stability assertion
- **业务阈值过滤（如 `sharpness_min`）激活后测试 fixture MUST 用 high-variance pattern**：旧同色 `vec![fill[0]; 16*16*3]` PNG → laplacian_variance = 0 < `sharpness_min=100` → 触发 `filter_blurry` 致 `grouped=0` 让测试断言 `report.grouped == 1` 失败；改 64×64 noise pattern（`noise_byte(i) = (i.wrapping_mul(37) ^ (i >> 3)) as u8`）让 sharpness 自然超阈值；同 seed 让两图 phash 仍同组（pHash 对绝对亮度不敏感）。新增"业务字段消费配置"commit MUST 同步更新依赖此配置默认值的旧 fixture，否则 grouped=0 类静默回归

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
- **新增容器/格式 EXIF 自解析** → `entities/<container>.rs`（chunk/segment 遍历）+ 调 `entities/tiff_ifd::parse_tiff`（PNG/JPEG header 入口）或 `parse_ifds`（裸 IFD 入口如 AVI strd）+ `entities/exif/image_<container>.rs` 或 fallback 接入点 + `entities/exif/types.rs::from_reader` 分流（image MIME 前置 / 视频 MIME 类似 `video/x-msvideo` 模式）+ 合成 fixture 与 `tests/fixtures/gen_<container>.py` Python 生成脚本入 `tests/fixtures/gen.sh`；**双 0 XMP fallback 调 `populate_image_xmp_fallback_if_empty`** 而非内联 `if dto==0 && create_date==0`（`image.rs` 主路径 + APP1 fallback + `image_png.rs` PNG eXIf 三调用点共享单 BR 收敛，避免 multi-instance 累加下 sub-branch 0-hit）
- **新增 office 容器/格式（PDF/OOXML/CFB/iWork/ODF/RTF/EPUB/思维导图族）** → `entities/office/<container>.rs` 子模块（`parse(reader, mime)` 入口 fn `#[cfg_attr(coverage_nightly, coverage(off))]` + 业务抽 `extract_dates(buf: &[u8])` 纯 helper 让 lib unit 测全分支）+ `entities/office/mod.rs` MIME 常量 + 路由 if-else 分支 + 必要时 `is_*_mime` helper + `entities/exif/mime.rs::is_office_mime` 加 mime + `entities/exif/mime.rs::mime_from_ext` 加扩展名映射 + `tests/lib_tidy/run_cli_flags.rs::OFFICE_FIXTURES` 加 fixture 文件名让 bin instance 自动遍历（消除 multi-instance phantom miss）+ `tests/fixtures/gen_<container>.py` Python 脚本（CFB 无 Python 写库，lib unit 测试内用 `cfb` crate 即时合成 fixture，不入 git）+ e2e 集成测试 `tests/lib_tidy/office_archive.rs` 验证归档桶
- **新增子命令（如 `move-text-shot`/`cull`）** → `adapters/cli.rs::Commands` enum 加 variant + `adapters/dispatch.rs::CommandResult` 加 variant + `tidy()` partial-failure arm + `tidy_with` match arm + `dispatch_<sub>` 函数 + `usecases/<name>/` 目录 + `usecases/report.rs::Report` 加 variant + `adapters/report_sink.rs::FEATURE_<NAME>` 常量 + match arm + `lib.rs` re-export Report 类型 + e2e MUST 含 `run_cli(["tidymedia","<sub>",...])` 字符串形式 + **若主流程含 detector `load_runnable` 失败 → `record_failure + continue` 模式（subprocess 真 bin instance 永不到内层 BR 比较）→ 对私有 helper 写微测试 + Fake 注入触发各 `?`/let-else Err arm，避免靠 ignore-regex 排除整文件（参考 `cull/run_tests.rs` 的 `crop_face_bbox`/`crop_eye_around`/`u32_from_f32_clamped` 微测试 + `read_to_end` reader_error 注入）**
- **新增 path 拼接调用点（含 helper / use case / adapter）** MUST 用 `Location::join_path(segment)` 不直接 `loc.path().join(...)`，否则 Windows host 跑远端 backend 时 path 被注入 `\` 致 SMB/ADB/MTP 命令异常；纯 Local 路径计算（如 `copy/naming.rs` 逐段 `Utf8PathBuf::join` 防 `..` 注入）可保留 OS 原生分隔符
- **新增 Output Port trait（推理类 Gateway / 基础设施抽象）** MUST 落到内层：①推理类（face/ocr/embedding/...）→ `usecases/<feature>/mod.rs`（被 use case 直接消费的 Port 在 use case 层）；②基础设施类（`BackendFactory` 与 `Backend` 同性质）→ `entities/backend/`。具体实现（Fake + 真实）留 `adapters/<feature>/`。**反例**：把 trait 留 `adapters/` 会让 usecases 出现 `use crate::adapters::...` 反向依赖，破坏 CA 内向规则
- **新增 OCR detector / TextDetector trait 方法** → `usecases/ocr/mod.rs::TextDetector` 定义（Output Port 在 use case 层）+ `adapters/ocr/fake.rs::FakeTextDetector` 同步 + `adapters/ocr/tract_dbnet*.rs` 真实实现（`*_real.rs` 走 `--ignore-filename-regex` 排除，`tract_dbnet.rs` 因含 subprocess instance phantom miss 也整文件排除——见「测试与覆盖率」节）
- **新增 Face detector / `FaceDetector`/`FaceEmbedder`/`FaceMeshDetector`/`EyeStateClassifier` trait 方法** → `usecases/face/mod.rs` 4 trait + `FaceDetection` DTO 定义（Output Port 在 use case 层）+ `adapters/face/fake.rs` 4 Fake 同步 + 4 套 `tract_{scrfd,mobilefacenet,facemesh,eyestate}.rs(+_real.rs)` 真实实现（`*_real.rs` 走 `--ignore-filename-regex` 排除；若主体 `tract_*.rs` 出现 subprocess phantom miss 同 `tract_dbnet.rs` 一并加入 ignore-regex）+ `lib.rs` re-export 4 trait + 4 Fake（`#[doc(hidden)]`）
- **新增 `FaceConfig` / `OcrConfig` / `CopyConfig` 字段** → `usecases/config.rs::*Config` + `BackendConfig.{face,ocr}` + `frameworks/config.rs::sanitize_*()` 加校验分支（对齐 `sanitize_ocr` 套路）+ `sanitize()` 末尾调用 + `config.yaml backend.{face,ocr,copy}:` 子段（每字段 `${TIDYMEDIA_*:-默认}` env 兜底）+ `usecases/config.rs::tests::config_defaults_match_historical_constants` 加新字段默认值断言 + **MUST `rg "<field_name>" src/usecases/ src/adapters/` 验证有真实消费点**——`sharpness_min` 曾定义+sanitize+yaml 注释承诺"丢弃模糊图"但 cull pipeline 无消费点成死配置，新增字段不能只在 sanitize 与 default 出现
- **新增 face 推理算法常量 MUST 入 `FaceConfig` 不留模块顶 const**：审计套路 `rg "^const [A-Z_]+: f32" src/adapters/face/ src/usecases/cull/`。helper-style 私有 fn 接单字段 `ratio: f32` 而非 `&FaceConfig` 保持纯净便于微测试；调用方上一级取 `face_cfg.xxx` 透传
- **改 `FaceEmbedder` 等 face trait 接口维度**（如 `[f32; 512] → [f32; 128]`）→ trait 定义 (`adapters/face/mod.rs`) + 真实 impl (`tract_mobilefacenet.rs::EMBED_DIM` + `decode` 返回类型) + `fake.rs` (字段/构造/`impl` 三处) + 单测 (`tract_*_tests.rs` 函数名 + `ConstRaw.dim`) + 集成测试 (`tests/lib_tidy/cull.rs` 等所有 `FakeFaceEmbedder::new([0.0; N])`)。sed `'s/\[0\.0; <旧>\]/[0.0; <新>\]/g'` 全局批量是可靠套路。**新增消费 face embedding 的代码 MUST 用 `[f32; identity_cluster::EMBED_DIM]` 单点常量**而非字面量 `[f32; 128]`，避免维度迁移时漏改（参考 `cull/run.rs::ImageAnalysis.embeddings`）
- **新增 face/image tract 推理 preprocess MUST 用 `Cow<'_, RgbImage>`** 避免 `INPUT_SIDE` 已对齐时 `img.clone()` 全量复制：`let resized: Cow<'_, RgbImage> = if size_match { Cow::Borrowed(img) } else { Cow::Owned(resize(...)) };`（参考 `tract_mobilefacenet/tract_facemesh::preprocess`）；同上 `tract_<model>::decide` 等 io::Result 返 fn MUST `map_err` 不 `.expect()`，让 caller 经 `?` 传播
- **推理 trait（SCRFD/DBNet 类）preprocess→infer 的 metadata MUST 与 input 一起按值透传**：旧实现 `Arc<Mutex<Option<ScaleMeta>>>` 共享可变状态，并发 `detect_faces` 时线程 B 的 meta 覆盖 A 的 → A 用 B 的 scale/pad 逆映射输出错框，违反 `FaceDetector: Send + Sync` 契约。`meta: Copy` 入参随 input 透传单点不可共享。同 letterbox preprocess MUST 不带 `.min(1.0)` 限制 upscale（小 eye crop 不放大让 YOLOv8 anchor 几乎不激活致永远判睁眼；ultralytics 标准 letterbox 允许 upscale）
- **face embedding decode `slice.len()` MUST `!=` 严格匹配 `EMBED_DIM` 不仅 `<`**：旧 `< 128` 让 512 维 InsightFace `w600k_r50.onnx` 错配通过，截取前 128 维 L2 归一与正确 embedding 不同空间 → cosine 比较随机聚类同人多张照片判作不同身份；严格 `!=` + 错配提示文案（"check facenet_model_path: must be 128-dim MobileFaceNet, not 512-dim InsightFace variant"）
- **face 算法常数表**（ArcFace 5 点模板 / MediaPipe FaceMesh EAR + 嘴部索引）保存在 `usecases/cull/face_align.rs` + `face_scoring.rs` 顶部 const 注释；测试 fake landmarks MUST 非共线（如 `[[1.0; 2]; 5]` 让 `align_face` 矩阵奇异致整脸丢弃，触发非预期路径）
- **新增 use case 的 `Report.scanned` 字段** MUST 与 `CopyReport` 同口径 = walker 触达数（含 `failed` / `skipped_*` / 非媒体），**不是**成功入索引/解码数；在 walk 循环内 `report.scanned += 1` 增量累加而非末尾 `report.scanned = success_vec.len()`（参考 `usecases/cull/run.rs::scan_source`）
- **「best + 多 culled」型 use case 的 `culled[i].score`** MUST 与 `best.score_breakdown.total` 同口径（综合 total）；`pick_best` 抽 `Vec<ScoreBreakdown>` 与 indices 同长返回，culled_refs 按 `enumerate().filter(i != best_pos).map(|(pos,_)| breakdowns[pos].total)` 收集，禁单分量（如 sharpness）替代综合 total
- **`FindReport` / `MobileFindReport` 等 schema 改动** MUST 三处同步：① `usecases/report.rs` Record 定义 ② `frameworks/mobile.rs::MobileFindReport` 镜像字段（uniffi Record 同名 + 类型映射，`u64` → `size_bytes`） ③ `adapters/report_sink_tests.rs::sample_*_report` fixture + JSON `parsed["groups"][0]["size"]` 断言；漏一处让 mobile FFI / sink 测试编译失败但易被 `cargo nextest -- mod_x` 局部跑掉
- **TIFF / IFD ASCII 字段 cnt ≤ 4 时数据 inline 在 entry val 字段（4 字节）**，cnt > 4 才走 `base[val_offset..]`；`tiff_ifd::read_ascii(base, val_bytes: [u8;4], val_offset, cnt)` 同时持 inline + offset 两种 fallback；旧实现一律 `cnt<=4 → None` 让 DJI/LG/FUJI 等短 Make/Model（`"DJI\0"` cnt=4、`"LG\0"` cnt=3）静默丢失

## 项目分层（Clean Architecture）
- 四层（自外向内）：`src/frameworks/` → `src/adapters/` → `src/usecases/` → `src/entities/`
- `bin/tidymedia.rs` **只**调 `tidymedia::run_cli(env::args_os())`，零业务；`lib.rs` 仅模块声明 + re-export
- `usecases/` 仅依赖 `entities/`；外层（frameworks）功能需被内层用时走 fn pointer 注入而非 `pub use` 桥接（见下条 config 范例）
- **跨层桥接消除推荐 fn pointer 注入而非 `pub use` re-export**：内层（usecases）持 `static LOADER: OnceLock<fn() -> T>` + `pub fn install_loader(fn)`，外层（frameworks）启动期调 install 装 fn 把实现绑上；保持源代码依赖向内单向。参考 `usecases::config` ↔ `frameworks::config::load`：旧 `usecases::config` `pub use frameworks::config::config` 桥接被替换后，usecases 不再 import frameworks，CA 核心检验通过
- `entities/backend/` 是 Gateway 抽象（`trait Backend` + `trait BackendFactory` + `SmbTarget`/`AdbTarget`/`MtpTarget`）；具体实现 `LocalBackend`/`DefaultBackendFactory` 等在 `adapters/backend/`；`file_info`/`file_index`/`exif`/`media_time::sidecar` 都 backend-aware（持 `Arc<dyn Backend>`）
- 目录名是 `usecases`（无下划线）
- **`tidy()` partial-failure Err 与 `tidy_with()` 不对称**：`tidy()`（CLI 入口）在 `CopyReport.failed > 0` 时返 Err 让 `$?` 非 0；`tidy_with(factory, ...)` 直接返 `CommandResult` 跳过该检查。覆盖 `dispatch.rs` partial-failure Err arm MUST 用 `tidy(Commands::Copy {..})`，不能用 `tidy_with`（参考 `tests/lib_tidy/move_failure_recovery.rs::tidy_returns_err_when_copy_partial_failure`）
- **Copy/Move partial-failure 文案按 `CopyReport.remove` 分流**：`dispatch.rs::tidy()` 内 `let op = if r.remove { "move" } else { "copy" }; let past = if r.remove { "moved" } else { "copied" };`，否则 Move 命令报错文案错写 `"copy partial failure"` 让 CI 脚本误判子命令；`adapters/report_sink.rs` 同步 `FEATURE_COPY` / `FEATURE_MOVE` 双常量按 `r.remove` 分流，避免结构化日志把 Move 失败事件归入 `feature="copy"`
- **新增 `if r.remove { ... } else { ... }` 等条件文案分流 MUST 双向各 1 测试命中**：单方向测试只覆盖 1 sub-branch，lcov BRDA 漏另一 arm；适用所有「按 enum 字段切文案/枚举」的 if-else（参考 `tests/lib_tidy/move_failure_recovery.rs::tidy_returns_err_when_{copy,move}_partial_failure` 对偶套路）

## 核心算法：media_time
- 优先级：**P0** = `ExifDateTimeOriginal` / `QuickTimeCreationDate` / `MkvDateUtc`；**P1** = `ExifCreateDate` / `QuickTimeCreateDate`；**P2** = 文件名启发式；**P3** = `XmpSidecar` / `GoogleTakeoutJson`；**P4** = `FsMtime`
- mtime 比 P0 早 > 30 天发提示性冲突告警
- **多数派仲裁**：filename 与 mtime 互证（差≤1天）且与 P0 差>30天 → 推翻 P0（相机时钟错场景，`ConflictKind::P0OverruledByMajority` 不静默）
- **ModifyDate 三方互证否决**：互证成立但 filename 票与 EXIF `ModifyDate` 差≤1天 → 判 filename+mtime 为 re-save 时戳，保 P0 记 `MajorityVetoedByModifyDate`（`ModifyDate` 进 `Exif.modify_date` 但不进时间候选，仅作仲裁旁证）
- `entities/media_time/` 7 子模块单一职责：`priority`（`Source`+`Priority`）/ `candidate`（`epoch_to_candidate`：secs==0 视未填返 None）/ `filename`（P2：`IMG_`/`DSC_`/`Screenshot_` 前缀 + 13 位毫秒戳 + WeChat/WhatsApp/Pixel/裸 15 字符 `YYYYMMDD_HHMMSS` + 通用 `<任意前缀>YYYY-MM-DD HH-MM-SS` 19 字节窗口 + **宽松 YYYYMMDD**（stem 开头或 `-`/`_`/空格 后 8 位合法日期）） / `filter`（`EPOCH_1904`/`SOFT_THRESHOLD_1995`/`FUTURE_TOLERANCE_SECS`）/ `resolve`+`decision`（多候选裁决）/ `fs_time`（P4）
- P3 sidecar 在 `adapters/sidecar.rs`（Interface Adapter，不在 entities）：XMP `photoshop:DateCreated` 纯文本搜索 + Takeout `<media>.<ext>.json` `photoTakenTime.timestamp` 走 `serde_json`；backend-aware，sibling 路径计算当前仅 Local
- **P0–P4 全链接线 `Info::create_time`**：P0/P1 来自 EXIF；P2 由 `candidates_from_filename` 消费（文件名 naive 按 `timezone_offset_hours` 解释，与 EXIF naive 同口径，**勿按 UTC** 否则月末晚间 +8 跨月归错桶）；P3 经依赖倒置注入——`dispatch` 把 `adapters::sidecar::discover_with_backend` 传给 `usecases::copy_with_sidecar`，`Index::enrich_candidates` 并行调 provider 填 `Info::add_candidates`；P4 = mtime。**e2e fixture 注意**：含 P2 文件名模式或带 `.json`/`.xmp` sidecar 的文件按文件名/sidecar 时间归档；`copy()`（无 provider）是 `#[cfg(test)]` shim，生产只走 `copy_with_sidecar`
- **EXIF vs 归档桶对账**：EXIF naive 按 `timezone_offset_hours`（默认 +8）转 epoch、归档再 `.to_offset(+8)` → 首尾抵消，**归档桶 = EXIF 字符串前 7 字符 `YYYY:MM`**，对账无需算时区。slash command `/tidy-verify <source> <output>`（`.claude/commands/tidy-verify.md` + `.claude/scripts/tidy-verify/`）：dry-run → exiftool 抽 → 桶 MISMATCH → 文件名时间 DIFFER → exiftool 修 → 真跑 move
- **QuickTime/视频容器时间是 UTC，对账须 +tz——「前 7 字符」规则仅对 EXIF naive 成立**：nom-exif 走 UTC epoch 再 `.to_offset(+tz)`，与 EXIF 首尾抵消口径不同；exiftool 默认 raw UTC 显示 QT 时间且**不加时区后缀**，`compare_buckets.py` 对 QT 列（`QT_COL_INDICES=(2,3)`）必须 `_qt_bucket()` 按 UTC→配置时区转换后取年月（带 `±HH:MM`/`Z` 后缀按其 offset 归一化）；否则月末 video 被误判 `TidymediaContainerMiss`。`03_compare_buckets.sh` 从 env > `config.yaml` 的 `timezone_offset_hours` > 8 解析时区
- **tidy-verify dry-run 年月分析**：直接按 `source=`/`target=` 父目录聚合会把根不同误判全变；MUST 剥源/目标根前缀到相对路径再比年月段
- **相机出厂默认时间陷阱**：EXIF P0 格式合法但值为出厂默认（如 2004-01-01 00:00），且早于机型发布日（FinePix E550 = 2004-07、Casio EX-Z50 = 2004-09）即判时钟未设；mtime 通常同错，多数派救不了，只能 exiftool 数据侧修复后再归档
- **多数派仲裁仅认 `Validity::Valid` 候选**：`apply_filters` 保留 `LowConfidencePre1995` 进 surviving；`majority_override` MUST 显式 `matches!(v, Validity::Valid)` 过滤低置信票，否则 1995 前 filename+mtime 互证可推翻可信 P0
- **冲突 `diff_secs` 约定** = `chosen.utc - other_utc`（`detect_conflicts` 各分支统一）；Override 分支 chosen=winner、other=原 P0 best；写反让 tidy-verify 判相机时钟方向颠倒
- **`EPOCH_1904` 过滤**用 `[EPOCH_1904, EPOCH_1904+86400)` 整天窗口——少数编码器把 `mvhd.creation_time` 写 1/2/... 让 ts=EPOCH_1904+1 漏过精确匹配
- **`Screenshot_` 截图两主流命名**：`yyyy-mm-dd-HH-mm-ss`（19 字符，Windows Snip & Sketch）与 `yyyymmdd_HHMMSS`（15 字符，Samsung/MIUI/原生 Android），`try_screenshot` 都要支持，否则后者退到 `try_loose_yyyymmdd` 兜底退化为日精度丢时分秒
- **copy/move 重叠保护**（`copy/run.rs`）：source ⊆ output → InvalidInput 拒绝；output ⊂ source（就地归档）→ `Index::remove_under_prefix` 剔除。前缀比较用 `entities::common::under_prefix`（分隔符边界）+ `entities::common::canonical_prefix`（Local `full_path` canonicalize / 远端 display）；**`canonical_prefix` 是 copy/move/cull/move_text_shot 4 个 use case 单点**，新增 source/output 类 use case MUST 用此 helper 替 `Location::display()`（output 是 symlink 时字面不匹配致 move 循环搬迁自身）

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
- feature off 返 `Unsupported`，`factory.rs` 与 `smb/adb/mtp::new()` 双路径 MUST 共走 `remote::unsupported_backend(feature)` 单点 helper，**不要手写文案**（避免分裂）
- 测试侧 `tidy_with(factory, command)` 接 `BackendFactory` 注入；集成测试用 `FakeBackendFactory`；`FakeBackend` / `FakeOp` 已 `#[doc(hidden)] pub use` 到 crate 根
- **`Backend::copy_file` 严格 reject 跨 scheme src/dst**：所有实现校验 src/dst scheme 必须等于自家 scheme（`local.rs::copy_file_rejects_non_local_*_scheme` / `adb_ops_tests::copy_file_rejects_non_adb_scheme_on_either_side` 印证）。跨 scheme（如 source=adb / output=local）MUST 走 `src.open_read` + `dst.open_write` + `io::copy` + `writer.finish` 流式 fallback；同 scheme 仍用 backend 原生 copy 享受同卷 rename / SMB `SRV_COPYCHUNK`。参考 `usecases/cull/group_writer.rs::copy_file_cross_scheme`
- **远端 backend path 拼接禁用 `Utf8PathBuf::join`**：Windows host 上 `Utf8PathBuf::from("/d").join("a.jpg")` 产 `/d\a.jpg`（std `PathBuf::push` 走 `MAIN_SEPARATOR='\\'`），SMB pavao / ADB shell `rm`/`mkdir -p` / libmtp 都把 path 字符串原样下发让远端找不到路径。**统一用 `Location::join_path(segment)`**（`entities/uri.rs`）按 scheme 分流：Local 走 `Utf8PathBuf::join`（OS 分隔符 fs 需要），远端（Smb/Mtp/Adb）始终用 `/` 字符串拼。新增 path 拼接点 MUST 用此 helper；segment 可含多段（`year/month`）。FakeBackend 用 `path.as_str()` 严格相等 HashMap key 模拟真实客户端，单测能自动捕捉该 bug

### 远端 backend 测试套路（SMB / MTP / ADB 通用）
- **手写 Fake\<Smb|Mtp|Adb\>Client**：state 用 `Arc<Mutex<HashMap<...>>>`；`inject(*Op::Read, path, ErrorKind::TimedOut)` 注入逐 op + 逐 path 错误，无须 `mockall`
- 错误文案映射：SMB / ADB `map_error` MUST 同时支持 **`NotFound`**（`enoent` / `no such file` / `does not exist`）+ **`PermissionDenied`**（`eacces` / `permission`）两类 ASCII 文案重映射，否则真实远端 `io::Error::other("pavao: ENOENT ...")` 让 `mkdir_recursive` 的 `NotFound` guard 永不触发 → 多层归档目录创建必败。**Fake 干净 `ErrorKind::NotFound` 会掩盖该缺陷**——专测 MUST 用 `FakeRemoteClient::with_error_factory(|k| match k { NotFound => Error::other("pavao: ENOENT ...") })` 模拟生产文案；匹配 MUST `to_ascii_lowercase()` 而非 `to_lowercase()`（避免 Unicode full-case folding 在本地化消息上字节漂移）
- **`map_error` 不能仅判 `kind == Other`**：`adb_client` 等链式错误可能返 `BrokenPipe`/`ConnectionReset` + 文案含 "no such file"；MUST 先放行已正确分类的 kind 再按文案重映射其余，否则 `exists()` 把"找不到"当 IO 错误传播
- **远端 helper 内调 `client.stat/mkdir/...` 也要 `map_err(A::map_error)`**：错误分类仅靠 `RemoteBackend::map_and_log` 单点不够——`mkdir_recursive` 等 trait 方法外的 raw client 调用未映射，pavao 把"路径不存在"包成 `Other("no such file")` 让 `e.kind() == NotFound` 守卫永远 miss
- 真实 client 适配器 `*_real.rs` 走 ignore-regex 排除（见「测试与覆盖率」节）；调度逻辑（`build_target` / `map_*_error` / `tidy_with` 各分支）走 fake 注入 100% 覆盖
- 每方法测三类：OK / client Err / 非自家 scheme 返 `InvalidInput`
- **"未启用 feature 返 Unsupported" 集成测试 MUST `#[cfg(not(feature = "<scheme>-backend"))]` gate**，否则启用 feature 跑 nextest 会 fail
- 远端 op 失败的结构化日志统一在 `remote.rs::map_and_log`（非泛型单点，避免 monomorphization 覆盖率重复计数）；新增远端日志沿用此单点，**勿**在 smb/adb/mtp 各自加
- **`map_and_log` 按 `io::ErrorKind` 分流级别**：`NotFound` / `AlreadyExists` → `debug!`（`exists()` 探测 + `mkdir_recursive` 容忍高频预期态，避免默认 info 刷屏）；其余（`PermissionDenied` / `TimedOut` / IO 链）→ `warn!`（用户感知业务错误必可见）。两 arm `debug!`/`warn!` 字段完全一致是合规重复（tracing macro level 必字面量、`event!(level, ...)` 不接运行时变量），LLVM 单 instance 累加无 BR sub-branch miss
- **feature off stub `new()` 用 `remote::unsupported_backend(feature)`**：smb/adb/mtp 三个 backend 的 cfg-off `new()` 共用单点 helper 构造 `io::Error::Unsupported`，文案 `"<feature> not enabled; rebuild with --features <feature>"`。新增 remote backend 用此 pattern 避免 stub 副本散落
- **adb shell `unlink` MUST 用 `rm` 不带 `-f`**：`-f` 让 ENOENT/路径错配 exit=0 静默成功致 move 模式 src 被认为已删；不带 `-f` 让 ENOENT exit=1 + stderr "No such file" 走 `map_remote_error` 重映射为 `NotFound` 让 caller 区分（与 SMB pavao ENOENT 同口径）
- **adb shell `check_shell_exit` `Option<u8>` `None` MUST 视 Err**：`adb_client::shell_command` 在连接断开/异常退出时返 `None`；旧 `unwrap_or(0)` 等价为 exit=0 让 unlink/mkdir 静默返 Ok。改 `match { Some(0)=>Ok, Some(n)=>Err, None=>Err("exit=unknown") }` 三态
- **`RemoteClient::list` 不带 size 时 child file 二次 `stat()` 失败 MUST `?` 上抛**：旧 `self.stat(&child).map_or(0, |m| m.size)` 让 size=0 → `file_info::ensure_hashable` 当空文件 reject → 目标文件静默不归档；与「存在性查询 Err MUST 传播」同口径（SMB list_dir 是该陷阱主犯，CIFS server 单文件 PermissionDenied 让全 list 退化为部分丢失而非 Err）

## 配置与日志
- 运行时配置：`config.yaml`（项目根）+ `src/usecases/config.rs`，`config()` 返 `&'static Config`（`OnceLock` + fn pointer LOADER）
- **config 跨层注入**（依赖倒置）：`usecases::config::config()` 走 `OnceLock<Config>` + `OnceLock<fn() -> Config>` lazy init；frameworks 启动期 MUST 调 `install_config_loader()` 把 `frameworks::config::load` 装上，否则 lazy init 走 `Config::default` 静默错配。入口：`bin/main` 第一行 / `frameworks/mobile.rs` 每个 FFI export 顶部 / 集成测试切 `TIDYMEDIA_CONFIG` 后；lib unit 测试调 `crate::install_config_loader()`，集成测试调 `tidymedia::install_config_loader()`。`frameworks/config_load_tests.rs` 直接调 `load()` 私有 fn 不走 OnceLock 不需 install
- 切换 `TIDYMEDIA_CONFIG=/path/to.yaml`；`${VAR:-default}` 由 `expand_env` 自实现（不引 dotenv）；嵌套 `{}` 默认值按括号配对解析；展开后以 `{` 开头的值（如 `archive_template`）yaml 中 MUST 加引号，否则被当 flow mapping 致整个配置回退默认
- 非法值校验走 `frameworks/config.rs::sanitize`（warn + 回退默认，与 parse 失败回退 `Config::default` 同哲学；已覆盖 `unique_name_max_attempts==0`、非法 `archive_template`、非法 `log.level`）；新增可校验字段在此加分支，勿在消费点散写
- **`sanitize` 在 `install_logging` 之前由 `OnceLock` lazy init 触发** → tracing subscriber 未装让 `warn!` 投 no-op dispatcher 被丢弃；关键 fallback（`log.level` / `archive_template`）用 `eprintln_sanitize_fallback` 走 stderr 兜底保 user 可见，业务热路径仍走 tracing（项目 R3 例外）
- **`expand_env` 与 `resolve_var` 相互递归 MUST 有 depth 上限**：`${A:-${A:-${A:-...}}}` 无限嵌套让栈爆；`expand_env_depth(input, depth)` 入参守 `EXPAND_ENV_MAX_DEPTH=32`，超界返字面量 + warn 不爆栈
- **env value 拼回 yaml 文本前 MUST `sanitize_env_value` 剥换行/控制字符**：`export VAR=$'info\narchive_template: "wrong"'` 让换行注入新顶层 key 覆盖原 `archive_template`；剥 LF/CR/NUL + 其他控制字符（保留 TAB）+ warn 一次。env value 直接 string 替换回 yaml 文本前 MUST 净化否则结构注入
- 结构化日志字段约定：`feature` / `operation` / `result`（CLI 无 request_id/user_id）
- `UtcOffset::from_whole_seconds` 范围 ±25:59:59，越界 None，用 `.unwrap_or(UtcOffset::UTC)` 兜底
- **R1 外置**：`copy.{timezone_offset_hours, unique_name_max_attempts, archive_template}` / `exif.valid_date_time_secs` / `backend.smb.{default_user,workgroup}` / `backend.adb.{server_host,server_port}` / `log.level`（CLI `--log-level` 缺省值，优先级 `RUST_LOG` > flag > 配置）。**无消费点的字段勿先加占位**
- **不外置的合理例外**：算法常量（`EPOCH_1904` / `SOFT_THRESHOLD_1995` / `FUTURE_TOLERANCE_SECS` / `MTIME_VS_P0_HINT_SECS`）/ 协议字面量（`IMG_` / `DSC_` / `Screenshot_` / `XMP_KEY`）/ 日志维度名 / lookup 表（`MONTH`）/ 流式哈希（`FAST_READ_SIZE`、`STREAM_CHUNK = 1 MiB`、`MIME_SNIFF_BYTES = 256`）
- `src/usecases/copy/ops.rs` 的 `println!("\"{}\"\t\"{}\"", src, dst)` 是 CLI 脚本可读输出，**不是** R3 日志路径，不要改成 tracing
- **环境变量速查**（章节散见，集中索引）：

  | 变量 | 用途 | 默认/兜底 | 说明位置 |
  |---|---|---|---|
  | `TIDYMEDIA_CONFIG` | 指定 config.yaml 路径 | `./config.yaml` | 本节首条 |
  | `RUST_LOG` | 日志级别（优先级最高） | 由 `log.level` 兜底 | 本节 R1 外置条 |
  | `CARGO_PROFILE_RELEASE_OPT_LEVEL` | e2e 真跑切 opt=3 | `0`（仓库 profile） | Quick Start §ONNX |
  | `TIDYMEDIA_OCR_DET_MODEL` | OCR DBNet onnx 路径 | `backend.ocr.det_model_path` | 系统依赖 §move-text-shot |
  | `TIDYMEDIA_FACE_*` | face 4 模型路径 + 阈值 | `backend.face.*` | 同步检查点 §FaceConfig |
  | `SMB_USER` / `SMB_PASSWORD` | SMB 凭据 | `backend.smb.default_user` / 无 | 凭据节 |
  | `KRB5CCNAME` | SMB Kerberos cache | 系统默认 | 凭据节 |
  | `ANDROID_HOME` / `ANDROID_NDK_HOME` | Android 交叉编译 | 无 | Android 节 |

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
- **跨平台 path 段断言推荐 `std::path::Path::ends_with`** 优于 `s.ends_with("a/b") || s.ends_with(r"a\b")` 双分支：std `Path::ends_with` 按路径段比较且 Windows 同视 `/`/`\` 为分隔符（std 文档保证），单条断言带诊断 message 即可，如 `assert!(std::path::Path::new(loc.path().as_str()).ends_with("out/2024/05/group-001"), "got: {}", loc.path())`。项目内 `file_info_tests.rs:192` 旧 `||` 模式合规但新写偏好 `Path::ends_with`
- **`cargo nextest run --release` 默认 fail-fast 让分散平台失败被截断**：首次 FAIL 后约 300+ 测试未跑只见 1 例。调试跨平台测试 MUST 加 `--no-fail-fast` 才能完整列举失败集（实测 Windows 5 cull failures 首跑只暴露 1 个 copy_generate_tests 因 fail-fast）
- **`Cargo.toml` 业务关键 dep MUST caret 锁主版本**（`sha2 = "0.11"` 而非 `"*"`），sha2 0.10→0.11 已踩坑；caret 让 patch 自由升 + 主版本手动验证才升。dev-deps 同口径；`generic-array = "0"` 等专项 transient dep 例外保留
- **本地 `[patch.crates-io]` 验证依赖 patch**：`cp -r ~/.cargo/registry/src/<idx>/<crate>-<ver>/ <vendor>` + patch + `[patch.crates-io] <crate> = { path = "..." }` → `cargo build` 触发重编。**Cargo.lock 中 patched crate 无 `source =`/`checksum =`** 即生效证据
- **远端 `RemoteClient::read` 整文件入堆**（已知限制，trait 文档有 WHY）：pavao `SmbFile` 借用 client 生命周期装不进 `Box<dyn MediaReader + 'static>`、`adb_client` pull 是回调式写入 API——真正流式需换库或线程+管道，YAGNI；大视频在内存受限环境（Android）有 OOM 风险
- **远端 `mkdir_p` 是真递归**（`remote.rs::mkdir_recursive`）：自底向上 stat 找存在的祖先再逐层 mkdir（`AlreadyExists` 容忍、stat 非 NotFound 直接传播）。`FakeRemoteClient::mkdir` 不校验父目录，单层 vs 递归差异 fake 测不出来，须用「中间层注入错误/断言中间层 metadata」类测试钉行为
- **存在性查询 Err MUST 传播，MUST NOT `unwrap_or(false)`**：吞 Err 让后续 `open_write` truncate 覆盖已有文件、move 模式连源一起删；保守兜底方向是 `true` 而非 `false`。**`std::path::Path::exists` / `camino::Utf8Path::exists` 返 `bool`、内部吞 `PermissionDenied` 成 false 是同类陷阱**——`LocalBackend::exists` MUST 用 `path.as_std_path().try_exists()?` 让 Err 经 `?` 上抛
- **mkdir/exists 类 best-effort 缓存 set MUST `contains + insert` 而非 `insert-returns-bool`**：`if !cache.contains(&k) { try_op(&k)?; cache.insert(k); }` 让 try_op 失败时 `?` 上抛且 set 未污染；反例 `if cache.insert(k.clone()) { try_op(&k)?; }` 让 try_op Err 后 set 已含 k 致后续命中跳过 try_op 但实际状态未建立（mkdir_p 失败永驻"已建"假态致后续 write 缺父目录全失败）。`do_copy::mkdir_cache` 与未来 dir-listing cache / sidecar exists cache 均适用
- **测试 shim 必须 `#[cfg(test)]` gate**：`Info::from` / `Index::visit_dir` / `Exif::from_path_with_offset` / `adapters/backend/fake_remote` 是包 backend-aware API 的旧入口，仅测试用；未 gate 让 release build 报 `dead_code`
- **`#[cfg(test)]` 标在方法/import 上，不要标在 `impl Foo {}` 块上**：同块生产方法会被一起 gate 掉
- **`--all-features` clippy 与 `#[cfg(not(feature))]` test 联动**：启用全 feature 后，gate 掉的 test fn 对应 imports 必须用同样 `#[cfg(not(all(feature = "smb-backend", feature = "mtp-backend", feature = "adb-backend")))]` 包裹，否则 `unused_import` error
- **clippy 1.95 `doc_markdown` 扩大**：含点号的文件名、含下划线的标识符在 `///`/`//!` 中均需反引号包；**首字母大写混合形态缩写**（`IoU`/`SoA`/`IoT`/`PaaS`）也命中，中文 doc 与英文混排时识别更激进（中文当 word boundary）
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
- **大文件 OOM fixture 用 random-noise PNG**：测 `max_image_bytes` 等 size 阈值分支需要真实大文件而非 `FakeBackend` 虚 size（API 不支持）；1500×1500 RGB random fill（如 `(i.wrapping_mul(37) ^ (i >> 3)) & 0xff`）经 PNG 编码后 > 1 MiB（噪声不可压缩），配合切独立 yaml `max_image_bytes: 1048576`（正好 1 MiB 不被 `sanitize_face` 拒绝）触发超限分支；纯色 PNG 压缩太小（< 1 KiB）无效
- **dry-run log / 路径聚合用 Python**（heredoc 或 `uv run xxx.py`）：
  - tidy-verify Step 3/4 已从 awk 迁到 `.claude/scripts/tidy-verify/*.py`（避 awk `\` regex 在 bash 单引号下二次解析致 `unterminated regexp`）
  - 临时一次性分析用 `cat > /tmp/tm/x.py <<'PYEOF' ... PYEOF` + `cd /tmp/tm && uv run x.py`
  - **项目根外独立脚本（如 `tests/fixtures/gen_*.py` fixture 生成）** MUST `uv run --quiet --no-project <script>` 否则 uv 在项目根检测 pyproject.toml 致行为不一致
  - **Windows 输出陷阱**：
    - Python stdout 默认 CRLF 与下游 grep/diff LF 不一致 MUST `sys.stdout.reconfigure(newline='\n')`
    - 终端 stdout 默认 GBK 把 UTF-8 中文显示成 `��` → 含中文/路径的报告 MUST 写 UTF-8 文件再读
    - Write 工具落盘含 `'\\'` 的 Python 字符串字面量会吞反斜杠（用 `SEP = chr(92)` 或 heredoc 绕开）
    - Python `re` alternation 是 leftmost-first 不是 POSIX leftmost-longest，迁 awk regex MUST 把长 token 放前（`(1[012]|0?[1-9])` 而非 `(0?[1-9]|1[012])`）
- **`tracing::*!` message 含 `{name}` 会被当 named placeholder**：`warn!(... "use ${VAR:-default}")` 让 macro 找名为 `VAR` 的变量编译失败；转义 `{{VAR}}` 合规但读怪，最干净改措辞避开 `{` `}`（如 `"use ':-default' suffix"`），字段引用走具名字段（`var = body`）而非 message 内嵌
- **`MediaWriter::finish` flush MUST `?` 传播不 `.ok()`**：当前 `std::fs::File::flush` 是 noop（std 文档保证），但 BufWriter 包装后 disk-full / cancel 在 finish 阶段才暴露，move 模式 src 随后删即永久丢失。新增 writer impl 同样要求；同款套路适用 `RemoteBufferedWriter::finish` 等 trait 实现
- **`BufWriter::into_inner` MUST 三阶段闭合**：BufWriter Drop 默认 flush + **忽略 Err**，disk-full / 远端 commit 失败在 Drop 静默丢失；显式 `bw.flush()?; let inner = bw.into_inner().map_err(IntoInnerError::into_error)?; inner.finish()?` 让 Err 经 `?` 传播。`into_inner` 内部会再 flush 但 buf 已空——`flush()?` 先暴露 Err，into_inner 成功后 inner 已 move 进 finish 路径，Drop 不再重入。与「`MediaWriter::finish` flush MUST `?`」同口径，专攻"BufWriter 包远端 writer"场景
- **`/code-review` agent 假阳性识别**：recall 模式过度报；「dead variant」先 `rg` 查测试引用 + 评估对称设计价值；「fake silent Ok」核对真实后端 POSIX 行为再判；「stub map_error」按未来接入价值评估。全文件审查不分派 `git diff` 时用 7 个 module-group agent 并行（backend / face+ocr / adapters顶层+uri / EXIF+容器 / file_info+media_time / usecases / frameworks+lib+bin），每 agent 返 ≤6 JSON findings，主线程独立 Read 验证
- **find 删除脚本统一 Python 输出**：`render_script` 输出 `#!/usr/bin/env python3` shebang + `import os` 头 + `os.remove("...")` 待删 / `# os.remove("...")` 注释保护，按 `DuplicateGroup` size 降序分组。**路径转义**：`s.replace('\\', "\\\\").replace('"', "\\\"")`；含 `\n` 控制字符暂不处理（YAGNI）
- **tract `outputs[i].cast_to::<f32>()` 返 `Cow<Tensor>` 不能直接 `.as_slice()`**：MUST 三步走 `let cow = ...?; let view = cow.view(); let slice = view.as_slice::<f32>().map_err(io::Error::other)?;`。`view().as_slice::<T>()` 返 `Result<&[T], TractError>`（不是 Option，`.ok_or_else` 编译失败）。`tract_dbnet.rs` 用 `.expect(...)` 掩盖了 API 形态——新接 tract 模型 MUST 用 `map_err` 让 Err 可传播；多输出模型（如 SCRFD 9 tensors）每个 tensor 独立 `view()` 持 binding 防 borrowed value dropped
- **clippy `field_reassign_with_default` (pedantic) 强制 struct literal**：测试 helper `let mut c = T::default(); c.field = v; c` MUST 改 `T { field: v, ..T::default() }`；仅覆盖部分字段时 `..T::default()` 合法，**全字段显式给值时禁用** `..Default::default()`（撞 `clippy::needless_update`）
- **f32 if-else 字面量类型推导陷阱**：`let x = if cond { 1.0 } else { 0.0 };` Rust 推 f64 默认；后续 `x.max(other_f32)` 报 `ambiguous numeric type {float}`。MUST 显式标 `let x: f32 = if ...`，或字面量后缀 `1.0_f32 / 0.0_f32`
- **P0 §1「MUST 用 `#[expect]`」仅约束 `clippy::xxx` lints，不约束 rust 内置 lint**（含 `dead_code` / `unused_imports` / `unused_variables`）：占位实现模块（plan 已注明 e2e 真跑后接入）用 module-level `#![allow(dead_code, reason = "占位实现：xxx 接入后启用")]` 合规；用 `#[expect(dead_code)]` 反而触发 `unfulfilled_lint_expectation` 当未来真接入时
- **分 commit 落地新算法模块同款 `#![allow(dead_code)]` 占位**：算法子模块（如 `cull/{face_align,identity_cluster,face_scoring}`）独立 commit 发布时尚未被消费 → 整模块 dead_code 噪声压住真错；接入 commit 同时删除 allow（与「新增整模块先做装配再写主体」同哲学的折中——装配可后置但占位 allow 必备）
- **新增整模块时先做 lib.rs re-export + dispatch 装配再写 algos 主体**：跳过装配链路直接写 algos 会让整个新模块所有 item 触发 `dead_code` warning（lib.rs 未 re-export → 子模块未 used → 链式传播），数十条 warning 噪声压住真实错误。装配链路（trait re-export + Fake re-export + Commands variant + CommandResult variant + dispatch_xxx + Report variant + sink match arm）先打通形成「死端到 lib API」通路，再写算法实现 build 干净
- **`epoch_to_candidate(0) → None` 把 0 视为 EXIF 字段未填，`fs_time::from_modified(UNIX_EPOCH) → Some` 是合法 fs 值**：两者守护逻辑同（u64 → i64 try_from + `TimeDelta::try_seconds` + `DateTime::checked_add_signed` 三段）但 `secs==0` 含义不同——`FakeBackend` 默认 mtime = `UNIX_EPOCH` 让 fs_time 必返 Some；fs_time 不能直接 delegate `epoch_to_candidate`，必抽独立 helper（`convert_secs_to_candidate`）加 `coverage(off)` 收敛 platform-bounded SystemTime 永不触发的 Err arm phantom miss
- **best-effort 路径 `if let Err(e) = ...` + `debug!`/`warn!` 抽独立 `log_xxx_err` helper 加 `coverage(off)`**：业务 `if let Err` 分支留原 fn 让测试可达（注入 Err → True arm 命中），tracing macro micro-region 0-hit 集中到 helper 排除；调用方测试用 fake inject 触达 Err arm 但断言「调用方仍 Ok」（debug 被 swallow），参考 `adapters/backend/remote.rs::mkparent` / `log_mkparent_err`
- **`infer` 把 OOXML/ODF/EPUB/iWork/思维导图 zip 容器一律识别为 `application/zip`**：`Exif::open` MUST 在 `sniffed=="application/zip"` 时与空 mime 同样走 `mime_from_ext(loc.path().extension())` 重映射到具体 office mime，否则 office 路由不命中 → doc_created=0 → 退到 mtime 桶（实测 docx 归档失败的根因）。新增 zip-based 容器同步 `mime_from_ext` 扩展名表
- **office 子模块 stub 签名用 `&mut dyn MediaReader` 而非 `Box<dyn>`**：stub 阶段 reader 不消费让 clippy `needless_pass_by_value` 抓不到；后续 commit 接入主体时 `zip::ZipArchive::new(&mut R)` / `cfb::CompoundFile::open(R)` / `read_to_end` 都接 `R: Read + Seek`，`&mut dyn MediaReader` 兼容，签名零改动
- **sed 批量加 doc_markdown 反引号陷阱**：`sed 's/PID_FOO/\`PID_FOO\`/g'` 会把 `const PID_FOO:` 定义本身改成 `const \`PID_FOO\`:` 编译失败；sed 后 MUST grep 校验 const / use / struct / enum variant 定义点未被错改（与「测试 env helper sed 自身递归」套路相同的预防原则）
- **`clippy::duration_suboptimal_units` (pedantic) 在测试 `Duration::from_secs(epoch_secs)`**：被建议改 `from_mins(epoch_secs/60)` 但 epoch 秒数语义直观 → 测试 module-level `#![allow(clippy::duration_suboptimal_units, reason = "Unix epoch 秒数语义直观")]` 合规
- **`serde_json::to_vec_pretty` 对 f32 NaN/Inf 字段返 Err 不写 null**：含 `ScoreBreakdown`/`f32` 字段的 manifest/report 生产路径 MUST `match` 处理 Err 而非 `.expect("never fails")`（cull manifest 已踩坑：上游 EyeState/FaceMesh NaN 经 unwrap_or 之外传染让 serialize 失败致 .expect panic）。best-effort 类落盘走 warn + early-return 与 `JsonFileReportSink` 同口径
- **phash 全纯色图（全黑/全白）median=0 致 hash 碰撞 u64::MAX**：DCT 高频系数对常数信号≈0，without_dc 中位数=0 让所有 bit 命中 `v>=median` 致 u64::MAX；`hash_from_block` 在 `median.abs() < EPSILON` 时短路返 0 让纯色图独立桶。改 median→mean 等算法时同样要测两张不同纯色图 Hamming 距离>0
- **`max_by` 选最大 f32 项 MUST 把 NaN 视为 -∞ 而非 Equal**：`partial_cmp().unwrap_or(Ordering::Equal)` 让 NaN==finite==NaN，Rust max_by 等值取末尾致 NaN 被选中 best（cull pick_best 已踩坑）。抽 `total_cmp_nan_as_neg_inf(a,b)`：双 NaN→Equal、NaN vs finite→Less、finite vs NaN→Greater，否则 partial_cmp。参考 `usecases/cull/crop.rs`
- **f32→u32 像素坐标 clamp MUST 三态拆**：NaN→0（无意义）/ 负数→0（下边界）/ +Inf→limit（上边界）。旧 `!v.is_finite() → 0` 一锅端把 +Inf 也归零让 SCRFD bbox 超界变 (0,0,0,0) 占位框；注释「超限→limit」与实现矛盾。参考 `usecases/cull/crop.rs::u32_from_f32_clamped`
- **clippy `neg_cmp_op_on_partial_ord`**：`!(a >= b)` 在 partial_ord 类型（f32 等）上拒绝；NaN-safe 守卫改 `a.is_nan() || a < b`（与 `!(s >= score_threshold)` 在 finite 路径等价 + NaN 显式跳过）。参考 `adapters/face/tract_scrfd_real.rs::decode_outputs`
- **「我的改动有没有破坏现有测试」验证套路** = `git stash && nextest -E 'test(<疑似失败>)' ; git stash pop`：本机 Windows 路径分隔符 / 平台差异 / nextest 顺序不稳定让本地基线 ≠ 0 常态，对比 main HEAD 同 filter 跑一次即知是否新引入；同 filter 失败数相等 = 我的改动无回归（即使 nextest summary 显示 fail > 0）
