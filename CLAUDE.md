@~/.claude/CLAUDE.md

@~/.claude/rust.md

# tidymedia 开发上下文

按「拍摄时间」去重整理照片/视频的多后端 CLI：sources（local/smb/adb/mtp 混合）→ SHA-512 去重 → 归档到 `output/年/月`。Clean Architecture 四层 + Android app（feature `android-app`）。

## Quick Start

- 优先使用 justfile
- 禁止直接使用 cargo

- **所有 cargo 命令 MUST 带 `--release`**（`profile.release opt-level=3`，统一 target 目录；快速编译验证用 `just OPT=0 build`）
- **ONNX 推理 e2e 默认即 opt=3**；日常测试用 Fake 注入
- **tract 加载分流**：4 face 模型（simplify 固化静态 shape）走 `into_optimized().into_runnable()`；PaddleOCR DBNet 动态 H/W 输入 MUST `into_typed().into_runnable()` 跳 optimize；bge BERT symbolic seq 维 MUST `into_typed()` → `set_symbols({batch_size:1, sequence_length:256})` → `into_optimized().into_runnable()`（`set_input_fact` 固化与图内 Unsqueeze 规则 unify 冲突，run 也不会自动绑 symbol）+ tokenizer encode 后手动 pad/truncate 到 SEQ
- **tract 0.23 `run` 接收者是 `self: &Arc<Self>`**：runnable MUST Arc 持有才能调 `run`；`into_runnable()` 已返 Arc，勿再 `Arc::new` 双包
- 典型归档 4 步：① `copy --dry-run --report /tmp/r.json` ② `/tidy-verify /src /out` ③ `copy` 真跑（或 `move` 删源） ④ `find /out` 兜底查重（输出 Python 脚本人工 review）
- **`copy-doc`/`move-doc`**：仅归档文档族（`is_office_mime` 全集），媒体/未知 skip；默认模板 `{category}/{year}/{month}` 触发 bge zero-shot 内容分类（`backend.classify.categories` 配类目原型文本，cosine < `score_min` 落 uncategorized）；模板不含 `{category}` 则整个分类阶段跳过（不加载模型）
- 测试 `cargo nextest run --release`；lint `cargo +nightly fmt && cargo clippy --release --all-targets --all-features --locked -- -D warnings`
- **超 2 min 命令（llvm-cov 全量等）**：macOS 无 `timeout`，用 `uv run --quiet --no-project python -c 'import signal,os,sys; signal.alarm(110); os.execvp(sys.argv[1], sys.argv[1:])' <cmd> <args>` 截断 + 重复执行步进推进（cargo 增量让每轮前进；被杀轮 exit=142/137，正常完成的 nextest 失败是 exit=100）
- **`--all-features` 三平台可构建**（smb-backend 已纯 Rust 化）；覆盖率权威口径仍 Linux CI
- CLI flag：`--log-level` 全局放最前；`--dry-run` 子命令级放 `copy`/`move` 后
- debug 走 stderr；`| tail -N` 会截 copy_file 行 → 重定向文件再 grep
- Windows：MSVC 链接固化在 `.cargo/config.toml`；全部工具链 MUST `x86_64-pc-windows-msvc`；vcvars64 未继承时 bash 内需 `.cmd` 脚本直执行（`cmd //c` 包装丢 VCToolsInstallDir）
- git commit：`type: 中文描述`（无 TASK-ID），按主题拆

## 系统依赖
- 无外部进程依赖；EXIF/视频走 `nom-exif` + `infer`
- **ONNX 模型走 git-lfs**（`models/` 4 face + 1 OCR + 1 bge ≈ 58 MB）：`scripts/download_models.sh` + `simplify_onnx.py` 装配；路径空时立即返 `InvalidInput`；HuggingFace LFS CDN 不可达（大文件超时、小文件正常）→ `HF_ENDPOINT=https://hf-mirror.com bash scripts/download_models.sh`
  - SCRFD-10G / MobileFaceNet **128 维**（非官方 512）/ MediaPipe FaceMesh 192×192 / YOLOv8 EyeState（**非 MobileNetV3 softmax**，640×640 letterbox，decode 取 `[1,6,8400]` anchor max closed conf）
- nom-exif 内部 tracing 大量输出，EnvFilter 默认压 `nom_exif=error`；GPS 子 IFD 用 `Exif::iter()` 按 tag code 匹配（`get()` 只读 IFD0）

### 容器解析补丁（nom-exif 不支持）
- **老 QuickTime `pnot`/`mdat` 首 box MOV**（2003-2008 早期机型）：`Cargo.toml [patch.crates-io] nom-exif = { git = "...", branch = "feat/legacy-quicktime-pnot" }`；入口用 `BoxHeader::parse` 仅解析 header（`BoxHolder::parse` 要整 body 入 buffer 必 Incomplete）
- **AVI（RIFF）**：拍摄时间在 `LIST hdrl > LIST strl > strd` chunk，`entities/riff.rs` 自解析；共用 `entities/tiff_ifd.rs`（II/MM + IFD0 + ExifIFD），双入口 `parse_tiff` / `parse_ifds`
- **PNG `eXIf` chunk**：Lightroom/相机直出常含，`entities/png.rs` 自解析；缺失或 TIFF 损坏退 XMP fallback
- **JPEG `parse_exif` 整体失败 fallback**：`image_jpeg.rs::parse_jpeg_app1_exif` 扫 APP1；三态闭合：fallback 成功直填 / Err 退 XMP / 主路径 Ok 但双 0 退 XMP
- **M2TS/AVCHD**：Canon 摄像机时间在 H.264 SEI MDPM UUID；AVCHD video PID `0x1011`；EP 字节 `00 00 03` 无条件 strip（H.264 spec）；仅填 DTO
- **JPEG/HEIC/TIFF/PNG XMP fallback**：re-tag 后 IFD0 仅剩 ModifyDate，原始时间在 XMP `photoshop:DateCreated`（→ P0）/ `xmp:CreateDate`（→ P1）；扫已 buffer 头 64 KB；seek 失败仍跑（head 已读入不浪费）；未闭合 `<!--` 按 strict 抹至 EOF
- **`tiff_ifd::scan_ifd` 是 lenient**：越界用 `break` 保部分字段，只有 `count` 本身读不到才返 `None`
- **TIFF ASCII cnt ≤ 4 inline 在 val 字段**（4 字节），> 4 才走 offset；旧一律走 offset 让 DJI/LG 短 Make/Model 丢失
- **`infer` 把 zip 容器（OOXML/ODF/EPUB/iWork）识别为 `application/zip`、CFB（doc/xls/ppt）识别为 `application/x-ole-storage`**（infer 的 CLSID 细分需完整 CFB header，256 字节 sniff 窗口必失败）：`Exif::open_filtered` MUST 对两者都走 `mime_from_ext` 扩展名重映射

## 测试与覆盖率
- **门槛 region/function/line/branch 四项全 100%**（Linux + `--all-features` + `--branch`）；**任何存量 miss 归属判定**（平台差异 / 疑似非本次引入）：同 filter `git stash -u`（含 untracked 新文件）对照 main HEAD 跑一遍
- **macOS 本机覆盖率已知缺口（勿追，Linux CI 口径 100%）**：`local.rs` 非 UTF-8 `?` edge（APFS 强制文件名 UTF-8，`fs::write` 返 EILSEQ 无法造 fixture）+ `remote.rs`/`entities/backend/mod.rs` 泛型 instance 宏 micro-region，main 基线即如此
- 严格 100% 命令：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch --ignore-filename-regex='(adapters[/\\]backend[/\\][a-z]+_real\.rs|adapters[/\\](ocr|face|classify)[/\\]tract_[a-z_]+\.rs|_phantom\.rs)$' --all-features`；`_phantom.rs` 是 per-instance phantom branch 所在函数的独立文件（函数已 100% 覆盖但某 instance 分支计 0，无法测试补，独立后整文件排除）
- **Windows 覆盖率两坑**（2026-08-30 实排）：① cargo-llvm-cov 0.8.7 的 `RUSTC_WRAPPER` 在 Windows + nightly 上解析 rustc 调用失败 → instrument 静默不注入（binary 无 `llvm_profile` 符号、profraw 零生成、merge 报 `not found *.profraw`），MUST `--no-rustc-wrapper` 改 RUSTFLAGS 直注（本机门禁走 rust-cov-100 skill gate，已内置该 flag）② lcov `SF:` 路径在 Windows 是反斜杠 → ignore-regex 必须 `[/\\]` 兼容写法否则排除组全失效
- `lib.rs`/`bin/tidymedia.rs` 顶 `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]`；`[lints.rust] unexpected_cfgs` 注册
- **覆盖率排除三组**：① `adapters/backend/*_real.rs`（大 + 需真环境）走 ignore-regex；② `adapters/(ocr|face)/tract_*_real.rs`（小）走 `#[coverage(off)]`；③ 主体 `tract_dbnet.rs` / `tract_{4face}.rs` / `tract_embed*.rs` 走 ignore-regex
- **子行 region miss 定位**：`llvm-cov report --release --text` 复用 profdata（`report` 不接 `--all-features`），`^0` 即 miss；branch miss 过滤 `BRDA` 第 4 字段 0；**function miss 用 `--lcov` 的 `FNDA:0`**（mangled 名可辨 closure/泛型；per-instance 有噪声，跨 instance 全 0 才是真 miss，summary 合并数为权威）
- 改 `Cargo.toml`/`coverage` 属性后必 `cargo +nightly llvm-cov clean --workspace`

### phantom miss 消除套路
- **tracing macro / best-effort `if let Err` 抽独立 helper 加 `coverage(off)`**：`debug!`/`warn!` micro-region 在 release subscriber 未订阅时 0-hit；业务 `if let Err` 分支留原 fn 让测试可达
- **`?` Err arm 彻底消除**：helper 直返 caller final result（`-> io::Result<Option<T>>`）caller `return helper(...)` 无 `?`；for-loop 内 `Ok(true)/Err(e) => return`, `Ok(false) => {}` 显式 match 替 `?`
- **assert 拆分**：`assert!(a && b)` → `assert!(a); assert!(b);`；`assert!(cond, "got: {}", expr)` panic 路径子表达式 0-hit → 提前 `let val = expr;`
- **fn pointer 依赖注入**：thin wrapper 生产 OK arm 触发 + Err arm 难触发 → 抽 `*_with(fn pointer)` 参数化让测试注 mock
- **私有 fn 加参数**：`#[cfg(test)]` shim + `use ... as 原名` alias 让 N 处测试零改动
- **多 callsite 同 `&&` 表达式**：抽 helper 收敛 BR（`if dto==0 && create_date==0` 三调用点共享）
- **`FakeBackend`**：`inject_reader_error` 首 read 即 Err、文案 = `io::Error::from(kind)` Display（不含"injected"），与 `inject_error` 走 `io::Error::other("injected ...")` 区分，测「sniff OK 后续 read Err」分段路径要抽 `read_to_end` 段到 `coverage(off)` helper；`copy_file` inject 按 **src loc** 匹配；`unique_name_*` 耗尽测试用 `add_file` 填满 N+1 候选替手写 wrapper
- **平台条件常量优先 `if cfg!(...)`**：跨平台行尾 MUST `#[cfg(target_os="...")] const X = "..."`；`cfg!()` 让 LLVM instrument 两 arm，dead arm 必 miss
- **私有 fn 微测试**（`#[cfg(test)] #[path] mod tests` + `use super::*;`）：命中 `||` 短路各 sub-branch + NaN/负值 clamp
- **office 子模块**：`parse(reader, mime)` 入口 fn + 复杂业务 fn 都 `#[cfg_attr(coverage_nightly, coverage(off))]`；正确性靠 lib unit `*_tests.rs` 全分支断言；不可达 `?` Err arm 改 `.expect("internal: ...")` 助手消除
- **subprocess office fixture 集中**：`tests/lib_tidy/run_cli_flags.rs::OFFICE_FIXTURES: &[&str]` 数组 + 单 test 自动遍历跑 `copy --include-non-media --dry-run` 让 bin instance 命中
- **超大常量守卫抽 pure helper**（`MAX_REMOTE_WRITE_BUFFER=2GiB`）让 test 传假 `u64` 直触各 arm
- **测试自身的平台差异 skip 分支**（如 `let Some(t) = pre else { return; }` Linux 恒不可达）：测试 fn 标 `coverage(off)` 从分母剔除

### 项目特有测试套路
- **fast-path 命中反向证据**：`fs::rename`/`fs::copy` 保留 src mtime，`std::io::copy` 不保留；`filetime::set_file_mtime` 钉 src 后 move 断言 dst mtime 不变
- **chmod 触发 IO 错误**：chmod 0 **parent dir** 才让 `try_exists`/`metadata` 返 `PermissionDenied`（chmod 0 文件 stat 仍成功）
- **测试 env helper**：模块顶 `#![allow(unsafe_code)]` + 单点 `set_env_var`/`remove_env_var`；sed 批量替换 MUST 避免匹配 helper 自身实现
- **Windows 同卷本地测**：`subst` + `mklink /J`；`.config/nextest.toml` `[test-groups.windows-volume-mut] max-threads=1` 强制串行；`subst Y: /D` RAII Drop guard
- **`--all-features` 覆盖**：`dispatch.rs` 的 `usecases::copy?` Err arm 用 output 父路径占普通文件触发 `mkdir_p` Err；feature 启用侧用 `#[cfg(feature="...")]` 镜像 `tidy_rejects_*`；`mobile.rs` 不可达防御抽纯 helper 直测
- **DCT pHash fixture**：≥ 256×256 大图 + random noise（非 gradient）+ JPEG 质量 90；brightness shift 断言 `Hamming ≤ 8`；纯色图 median=0 让 hash 短路返 0（否则 u64::MAX 碰撞）
- **业务阈值 fixture**（`sharpness_min`）：high-variance noise pattern（同色 laplacian_variance=0）
- **`config.yaml` 默认值改**：`OnceLock` 一次性加载 → 集成测试切独立 yaml + `TIDYMEDIA_CONFIG` env
- **`fake_remote::list` 返直属子项**（非 flat 子树）：`walk_recursive` 的 `EntryKind::Dir` 递归分支需 `add_dir` + `add_file` 组合驱动
- **`supports_native_rename_to` True 分支**：MUST 用真实 `LocalBackend` + tempfile 真跑 `fs::rename`
- **`epoch_to_candidate(0) → None`**（视 EXIF 未填）vs `fs_time::from_modified(UNIX_EPOCH) → Some`（合法 fs 值）语义不同：抽独立 `convert_secs_to_candidate` helper
- **验证「我的改动是否破坏现有测试」**：`git stash && nextest -E 'test(...)' ; git stash pop` 同 filter 对比 main HEAD
- **`FakeBackend.walk` 对不存在 root 静默返空**（`LocalBackend` 计 walker_errors=1）：「扫描完整性/权威性」类测试两者语义不同，勿用 Fake 断言 walker 错误路径

## 性能采集（AI 分析用）
- **一次性汇总**：`bun scripts/perf-collect.ts --sub <copy|move|find|cull|move-text-shot> --data <真实源目录> --output-dir <dir>` → 产 `report.json`（含 `duration_ms`）+ `time-v.txt`（`/usr/bin/time -v` 抓 RSS/CPU/IO）+ `perf-report.md`（单一 markdown 直接扔 LLM 分析）
- **产物机器可读**：不产 SVG 火焰图/二进制 pprof profile；深度剖析走 samply attach（补充手段，非默认）
- **详细指南**：`docs/performance.md`（字段字典 + AI 分析 prompt 模板 + 常见瓶颈判定路径）；用户问「性能怎么测」/ 「如何分析瓶颈」直接指到此文档

## Fixture
- `tests/data/` mtime 每次 `git checkout` 重置；时间测试 MUST 用 `filetime::set_file_mtime` 固定（`entities/test_common::copy_png_to` → `FIXED_MEDIA_MTIME` 2024-01-01 12:00:00 UTC）
- MP4 不传 `-metadata creation_time=` 时 nom-exif 返 `Some(1904-01-01)`；要 None 用 MKV
- `FakeBackend` 默认 mtime = `UNIX_EPOCH` → P4 兜底 + 默认 +8 时区 → 归档桶 `1970/01`
- `camino::Utf8Path` Linux 不识 `\` 为分隔符，Windows 反斜杠测试行为不同

## 文件组织
- 文件 ≥400 行预防性拆至 ≤300；测试外置 `#[cfg(test)] #[path = "X_yyy_tests.rs"] mod yyy_tests;`；生产目录化 `foo/mod.rs`（仅声明+re-export）+ 子模块
- 测试要访问的内部项在 mod.rs `#[cfg(test)] use self::sub::item;` 私有 re-export；跨子模块用 `pub(super)`；子模块 MUST NOT 命名 `core`
- **大文件多份 tests 外置**：共享 helper 抽 `<mod>_tests_common.rs`（`pub(super) fn`）；主 mod 顶层挂 `#[cfg(test)] #[path]` mod（**勿嵌套 `mod tests {}`**——`#[path]` 在嵌套下基于父 mod 目录解析）
- CA 依赖方向验证：`rg "use crate::adapters|use crate::frameworks" src/entities/ src/usecases/` 应仅返 re-export 桥接
- 集成测试拆分：`tests/<name>.rs` 是 root binary，子目录不当独立 binary；root 用 `#[path = "<name>/sub.rs"] mod sub;` 装配

## 同步检查点（改 X → 同步 Y）
> 字面默认值变更先 `rg <旧值>` 兜底改全

- **新增 `Location` variant / backend scheme** → `entities/uri.rs::FromStr` + `adapters/backend/factory.rs`（cfg-gated + Unsupported 兜底）+ Backend impl + `adapters/dispatch.rs` 调度 + 「URI 格式」
- **新增 `Backend` trait 方法** → 全部 7 实现同步：`local`/`remote`/`smb`/`adb`/`mtp`/`fake`/`fake_remote`；三类测试：OK / client Err 注入 / 非自家 scheme 返 `InvalidInput`
- **新增 `Backend` impl** → `walk` MUST 递归 yield 所有 file；远端 `walk_recursive` MUST 有 `visited: HashSet<String>` 防 symlink 环 OOM（ADB /sdcard loop / SMB DFS junction / mtp 挂载回环）
- **新增配置字段** → `usecases/config.rs` 结构体 + `config.yaml` + `frameworks/config.rs::sanitize_*` 校验 + `config_defaults_match_historical_constants` 测试 + `rg <field>` **验证有真实消费点**（防死配置）；secret 走 `.env.example` + gitignore
- **新增 CLI flag** → `adapters/dispatch.rs` 透传 + 每子命令路径独立 e2e 触发 Some/None 两边；e2e MUST 含 `run_cli(["tidymedia", ...])` 字符串形式
- **新增 `media_time` 候选 / 调整 P0–P4** → `entities/media_time/priority.rs` `Source`/`Priority` → 解析模块 → `resolve`/`decision` 裁决 → fixture；**新增下载时戳类 P2 variant MUST 加入 `Source::is_majority_filename_vote` 黑名单**
- **新增 archive_template 占位符** → `usecases/archive_template.rs::render` + 同文件 `PLACEHOLDERS` 常量 + `usecases/config.rs::validate_archive_template` 三处同步；用户值入路径段（如 `{category}` 类目名）MUST 过 `sanitize_path_segment`
- **新增容器 EXIF 自解析** → `entities/<container>.rs`（chunk 遍历）+ 调 `tiff_ifd::parse_tiff`/`parse_ifds` + `entities/exif/image_<container>.rs` 或 fallback 接入 + `types.rs::from_reader` 分流 + `tests/fixtures/gen_<container>.ts`；**双 0 XMP fallback 调 `populate_image_xmp_fallback_if_empty`** 单点
- **新增 office 容器** → `entities/office/<container>.rs`（`parse(reader, mime)` 入口 + `extract_text(reader, mime, max_bytes)` 文本提取 + 业务纯 helper lib unit 测全分支）+ `entities/office/mod.rs` MIME + 双路由（`populate_office_dates`/`extract_office_text`）+ `entities/exif/mime.rs::{is_office_mime, mime_from_ext}` + `OFFICE_FIXTURES` 数组 + e2e `tests/lib_tidy/office_archive.rs`；**fixture 进 `OFFICE_FIXTURES` 后该容器全部业务 fn MUST `coverage(off)`**（subprocess bin instance 只跑 happy path，multi-instance branch 记录让 lib unit 全分支覆盖被拆散成 phantom miss）；剥标签/截断共用 `entities/office/scan.rs`
- **新增子命令** → `Commands` enum + `CommandResult` variant + `tidy()` partial-failure arm + `tidy_with` match + `dispatch_<sub>` fn + `usecases/<name>/` 目录 + `usecases/report.rs::Report` variant + `report_sink.rs::FEATURE_<NAME>` 常量 + match + `lib.rs` re-export
- **新增 path 拼接调用点** MUST 用 `Location::join_path(segment)`（Local `Utf8PathBuf::join` / 远端 `/` 字符串拼）不直接 `loc.path().join(...)`：Windows host 上 std `PathBuf::push` 产 `\` 让 SMB smb2（反斜杠被映射 U+F026 普通字符）/ADB shell/libmtp 找不到路径；`SmbTarget/AdbTarget/MtpTarget.path` 内部拼子路径同理；纯 Local 计算保留 OS 分隔符
- **新增 Output Port trait** MUST 落到内层：推理类（face/ocr/embedding）→ `usecases/<feature>/mod.rs`；基础设施类（`BackendFactory`）→ `entities/backend/`；具体 impl 留 `adapters/`；**反例**：trait 留 `adapters/` 破坏 CA 内向规则
- **新增装配 Port**（factory 类，如 `DetectorFactory`/`BackendFactory`）→ trait 在内层（推理类 `usecases/<feature>` / 基础设施类 `entities/backend`）+ `Default<X>Factory` impl 在 `frameworks/<x>.rs`（"决定用哪个具体实现"归最外层；`adapters` 只留 Port 具体 impl）+ `dispatch::tidy_with` 签名增 `&dyn <X>Factory` 参数走 trait 消费 + `lib.rs` re-export `Default<X>Factory` + `<X>Factory` + 所有 `tidy_with` 调用点（含集成测试 ~30 处）扩参（Python `re` 脚本按多行/单行两模式批量改）
- **新增 face 算法常量** MUST 入 `FaceConfig` 不留模块顶 const；helper-style 私有 fn 接单字段 `ratio: f32` 保持纯净便于微测试
- **改 `FaceEmbedder` 维度** → trait + 真实 impl（`EMBED_DIM` + decode 返回类型）+ `fake.rs`（字段/构造/impl）+ 单测 + 集成测试所有 `FakeFaceEmbedder::new([0.0; N])`；sed 批量套路可靠。**MUST 用 `[f32; identity_cluster::EMBED_DIM]` 单点常量**
- **推理 metadata MUST 与 input 一起按值透传** 不用 `Arc<Mutex<Option<Meta>>>` 共享可变状态（并发 `detect_faces` 时错框）；letterbox preprocess MUST 不带 `.min(1.0)`
- **face embedding decode `slice.len()` MUST `!=` 严格匹配 `EMBED_DIM`**（`< 128` 让 512 维 InsightFace 错配通过截前 128 维错空间）
- **新增 `Backend` 能力查询** MUST 走 trait method 不硬编码 scheme：`fn supports_native_rename_to(&self, other: &dyn Backend) -> bool { false }` default
- **新增 tracing 结构化日志 `feature` 常量** MUST 从 `usecases::report` 单点 `use`（`FEATURE_COPY/MOVE/FIND/CULL/MOVE_TEXT_SHOT` + `feature_of(remove: bool)`）；禁本地 `const` 副本
- **`do_copy` 类 dry_run 分支** MUST `output_index.add(src.cloned_at(target, out_be))` + 前 `src.secure_hash()?` pre-populate：否则同 basename+同月+不同 hash 静默分派到同 target；且 `exists(secure=true)` 触发 `open_read(target)`，dry-run/rename 半态下 `NotFound` → 假失败
- **无 Index 的 move 类 use case（`move-text-shot`）入口 MUST 三层守卫**：① `sources ⊄ output` ② `sources` 两两 `under_prefix` 检查 ③ 空 file_name 走 `record_failure` 不 panic
- **移动类 rerun-safe 三态 `TargetDecision`**：base 已存在时 size 快过滤 → SHA-512 双侧比对 → `Duplicate`（删源计 `deduplicated++`） / `Fresh(_N)` / `Exhausted`
- **bytes-based use case**（OCR/图像）读源 MUST 拆两段：`MIME_SNIFF_BYTES(256)` sniff → 非目标 skip → 命中才 `read_to_end`（远端 `open_read` 整文件入堆）；`Vec::with_capacity(usize::try_from(entry.size).unwrap_or(0))` 精确预分配
- **Report `errors: Vec<ReportError>`** MUST 有 `ERRORS_SOFT_CAP=1000` + `errors_truncated: bool`；单点 `usecases::report::push_error_capped` / rayon merge 用 `extend_errors_capped` 传染 `src_truncated`；`failed` 不受 cap
- **`FindReport` schema 改动** MUST 三处同步：`usecases/report.rs` + `frameworks/mobile.rs::MobileFindReport` + `report_sink_tests.rs` fixture + JSON 断言
- **Copy/Move partial-failure 文案分流**：`dispatch.rs::tidy()` 按 `CopyReport.remove` 分 `op/past` 文案 + `report_sink.rs` `FEATURE_COPY`/`FEATURE_MOVE`；条件文案 MUST 双向各 1 测试
- **新增 `Report.scanned` 字段** MUST 与 `CopyReport` 同口径 = walker 触达数（含 failed/skipped/非媒体）；walk 循环内增量而非末尾 `= success_vec.len()`
- **新增 Report 时序/资源字段**（如 `duration_ms`）MUST 4 Report + `MobileFindReport` + 相应 fixture 五处同步；耗时统一走 `usecases::report::elapsed_ms(Instant)` 单点（`coverage(off)` 免宿主时钟波动断言）
- **「best + 多 culled」型 `culled[i].score`** MUST 与 `best.score_breakdown.total` 同口径（综合 total），禁单分量替代
- **新增 move 类「copy 成功但删源失败」半态路径** MUST 用 `entities/backend/partial_move.rs::partial_move_error` 构造（检测走 `is_partial_move` downcast），禁 `io::Error::new`+文案 `contains` 匹配；Display 文案仍 MUST 含 `copied ... but cannot remove source`
- **`generate_unique_name` 探测三层**：output_index `contains_target` → `index_authoritative`（output 扫描 stats 全 0 或 root 不存在，跳过 `backend.exists` 省远端每文件 1 RTT）→ `backend.exists` 兜底；改 walk/`VisitStats` 语义 MUST 复核 `run_copy_loop` 权威判定
- **copy-doc 分类管线单向数据流**：dispatch（`doc_only && template_needs_category` 才 `build_document_classifier`）→ `make_classify_provider`（阈值裁决）→ `Index::classify_documents`（gate `is_office` + 复用 parse_exif 已 sniff MIME，MUST 在 parse_exif 之后）→ `Info.category`（`cloned_at` MUST 搬运）→ `TemplateContext.category`（`unwrap_or("uncategorized")`）；`TextClassifyProvider` 是 `Box<dyn Fn>`（分类器有状态，裸 fn 指针装不下；Entity 签名只现 `std::ops::Fn` 保 CA 内向）
- **非媒体 EXIF 短路三态**：`Exif::open_filtered(..., parse_non_media, doc_only)` + `Index::parse_exif(offset, parse_non_media, doc_only)`——`doc_only=true` 反向短路（仅文档族整文件解析，copy-doc/move-doc 路径）；`Exif::open` 已是 `#[cfg(test)]` shim，生产新调用走 `open_filtered`；落盘过滤 `ops.rs::passes_type_filter` 与解析短路同口径

## 项目分层（Clean Architecture）
- 四层（外向内）：`frameworks/` → `adapters/` → `usecases/` → `entities/`
- `bin/tidymedia.rs` **只**调 `run_cli(env::args_os())`；`lib.rs` 仅模块声明 + re-export
- `usecases/` 仅依赖 `entities/`；跨层需求走 fn pointer 注入不 `pub use` 桥接
- **跨层桥接**：内层持 `static LOADER: OnceLock<fn() -> T>` + `pub fn install_loader(fn)`；外层启动期调 install。参考 `usecases::config` ↔ `frameworks::config::load`
- `entities/backend/` 是 Gateway 抽象（`Backend` trait + `BackendFactory` + `SmbTarget`/`AdbTarget`/`MtpTarget`）；具体 impl 在 `adapters/backend/`
- 目录名 `usecases`（无下划线）
- **`tidy()` vs `tidy_with()` 不对称**：`tidy()`（CLI 入口）在 `CopyReport.failed > 0` 返 Err 让 `$?` 非 0；`tidy_with` 直返 `CommandResult` 跳过检查

## 核心算法：media_time
- **P0** = `ExifDateTimeOriginal` / `QuickTimeCreationDate` / `MkvDateUtc`；**P1** = `ExifCreateDate` / `QuickTimeCreateDate`；**P2** = 文件名启发式；**P3** = `XmpSidecar` / `GoogleTakeoutJson`；**P4** = `FsMtime`
- mtime 比 P0 早 > 30 天发提示性冲突告警
- **多数派仲裁**：filename 与 mtime 互证（差≤1天）且与 P0 差>30天 → 推翻 P0（相机时钟错，`P0OverruledByMajority` 不静默）；**仅认 `Validity::Valid` 候选**（`majority_override` 显式过滤 `LowConfidencePre1995`）；**filename 票仅限拍摄命名类**（`Source::is_majority_filename_vote` 黑名单 `FilenameUnixMillis`/`FilenameWeChatExport`——下载时戳与 mtime 天然同源，互证恒真是假象，不得推翻 QT P0）
- **ModifyDate 三方互证否决**：filename 票与 EXIF `ModifyDate` 差≤1天 → 保 P0 记 `MajorityVetoedByModifyDate`（`ModifyDate` 仅作旁证不进候选）
- `entities/media_time/` 子模块：`priority` / `candidate`（`epoch_to_candidate` secs==0 视未填）/ `filename`（IMG/DSC/Screenshot 前缀 + 13 位 ms 戳 + WeChat/WhatsApp/Pixel/裸 `YYYYMMDD_HHMMSS` + 通用 19 字节窗口 + 宽松 YYYYMMDD）/ `filter`（EPOCH_1904/SOFT_THRESHOLD_1995/FUTURE_TOLERANCE_SECS）/ `resolve`+`decision` / `fs_time`
- **P3 sidecar 在 `adapters/sidecar.rs`**（非 entities，Interface Adapter）；backend-aware，sibling 路径当前仅 Local；经依赖倒置注入：`dispatch` → `copy_with_sidecar`；`copy()` 无 provider 是 `#[cfg(test)]` shim
- **文件名 naive 按 `timezone_offset_hours` 解释**（勿按 UTC）与 EXIF 同口径
- **`Screenshot_` 两主流命名**：`yyyy-mm-dd-HH-mm-ss`（19 字符 Windows Snip）+ `yyyymmdd_HHMMSS`（15 字符 Samsung/MIUI/Android）
- **`EPOCH_1904` 过滤用 `[EPOCH_1904, EPOCH_1904+86400)` 整天窗口**（少数编码器写 `+1/2/...`）
- **相机出厂默认时间陷阱**：EXIF 值合法但为出厂默认（2004-01-01）且早于机型发布日 → 判时钟未设；exiftool 数据侧修复后归档
- **冲突 `diff_secs` 约定** = `chosen.utc - other_utc`；Override 分支 chosen=winner、other=原 P0 best

### EXIF vs 归档桶对账
- EXIF naive 按 `timezone_offset_hours`（默认 +8）转 epoch、归档再 `.to_offset(+8)` → 首尾抵消，**归档桶 = EXIF 字符串前 7 字符 `YYYY:MM`**
- **QuickTime/视频是 UTC，对账须 +tz——「前 7 字符」规则仅对 EXIF naive 成立**：verify 已内化（`verify::bucket::qt_bucket` 对 QT 列 0-based idx 1/2 按 UTC→配置时区转换）
- `/tidy-verify <source> <output>` slash command（`.claude/commands/tidy-verify.md` 薄壳 → `.claude/skills/tidy-verify/SKILL.md`，脚本 Bun+TS）：dry-run → exiftool 抽 → `verify` 对账（stdout 内建汇总）→ exiftool 修 → 真跑 move；长步骤 MUST Monitor 后台（Bash 前台/`run_in_background` 2 分钟必被杀）
- **dry-run 年月分析**：MUST 剥源/目标根前缀到相对路径再比年月段

### copy/move 重叠保护
- source ⊆ output → `InvalidInput` 拒绝；output ⊂ source（就地归档）→ `Index::remove_under_prefix` 剔除
- 前缀比较用 `entities::common::under_prefix`（分隔符边界）+ `canonical_prefix`（Local canonicalize / 远端 display）；**`canonical_prefix` 是 copy/move/cull/move_text_shot 4 单点**，新增 use case MUST 用此 helper 替 `Location::display()`
- **entry 级 output 子树判定 MUST 用 `common::entry_under_prefix`（字面 fast-path + canonical 补判）**：canonical output prefix 与 walker 字面路径在 macOS `/var→/private/var`、`/tmp→/private/tmp` symlink 下不可比，纯字面 `under_prefix` 恒 false 让就地归档保护静默失效；copy 的 `Index::remove_under_prefix`（key 是字面 `&str`）走 canonical + 字面双前缀各剔一遍（幂等免相等判分支）
- **`canonical_prefix` vs `file_info::full_path` 语义正交**：前者解析 symlink 真实物理位置；后者路径索引 key 希望字面稳定（对 `is_absolute()` 跳过 canonicalize 避 Windows 跨盘）

## URI 与 Backend

### URI 格式
- CLI 接 `Location`（`FromStr`）：无 `://` 或 `local://` → `Local`；`smb://[user@]host[:port]/share/path`；`mtp://device/storage/path`；`adb://[serial]/abs/path`（serial 空则 autodetect）
- 字段空格/中文走 `percent-encoding`，**不引** `url` crate
- 混合 sources：`copy smb://a /local/b mtp://c adb:///sdcard/d -o /x`
- **IPv6 host 用方括号**：`smb://[::1]/share`；`split_host_port` 优先识别 `[...]`

### 凭据
- SMB：`SMB_USER` 经 `backend.smb.default_user` 兜底；`SMB_PASSWORD` 在 `build_target` 读 env；smb2 首期 NTLM only，`KRB5CCNAME` 已读入 `SmbTarget` 但 Kerberos 未接入（smb2 `KerberosAuthenticator` 独立装配路径）；**密码永远不入 YAML**
- ADB：`adb start-server` + 设备 USB 调试；多设备 URI 必须带 serial

### 工厂与远端 backend
- `DefaultBackendFactory` 按 `Location` 装配；MTP 当前 stub 返 `Unsupported`
- feature off `new()` MUST 走 `remote::unsupported_backend(feature)` 单点 helper 不手写文案
- 测试 `tidy_with(factory, command)` 注入；`FakeBackend`/`FakeOp` `#[doc(hidden)] pub use` 到 crate 根
- **`Backend::copy_file` 严格 reject 跨 scheme src/dst**：跨 scheme MUST 走 `open_read` + `open_write` + `io::copy` + `writer.finish` 流式 fallback

### 远端 backend 测试套路
- **手写 `Fake<Smb|Mtp|Adb>Client`**：state `Arc<Mutex<HashMap>>`；`inject(Op::Read, path, ErrorKind::TimedOut)` 无须 `mockall`
- **`map_error` 支持文案重映射**：`NotFound`（enoent/no such file/does not exist）+ `PermissionDenied`（eacces/permission）两类 ASCII；MUST `to_ascii_lowercase()` 非 `to_lowercase()`（避 Unicode full-case folding）；MUST 先放行已正确分类的 kind 再按文案重映射（`adb_client` 链式错误可能 `BrokenPipe`+"no such file"）
- **helper 内 raw client 调用**（`client.stat/mkdir/...`）MUST `map_err(A::map_error)`；仅靠 `map_and_log` 单点不够
- 远端 op 结构化日志统一在 `remote.rs::map_and_log`（非泛型单点）；按 `ErrorKind` 分流：`NotFound`/`AlreadyExists` → `debug!`（`exists()` 探测高频）；其余 → `warn!`
- **adb shell `unlink` 用 `rm` 不带 `-f`**（`-f` 让 ENOENT exit=0 静默）
- **adb shell exit `Option<u8>` `None` 视 Err**（旧 `unwrap_or(0)` 让异常退出静默）
- **`RemoteClient::list` 不带 size 时 child file 二次 `stat()` 失败 MUST `?` 上抛**（旧 `map_or(0, |m| m.size)` 让 size=0 → 空文件 reject 静默不归档）
- **未启用 feature 集成测试** MUST `#[cfg(not(feature = "<scheme>-backend"))]` gate

## 配置与日志
- `config.yaml`（项目根）+ `usecases/config.rs`；`config()` 返 `&'static Config`（`OnceLock` + fn pointer LOADER）
- **`install_config_loader()` 入口**：`bin/main` 第一行 / `frameworks/mobile.rs` 每 FFI export 顶 / 集成测试切 `TIDYMEDIA_CONFIG` 后；lib unit 调 `crate::install_config_loader()`
- 切 config 用 `TIDYMEDIA_CONFIG=/path/to.yaml`；`${VAR:-default}` 由 `expand_env` 自实现；嵌套 `{}` 按括号配对解析；展开后以 `{` 开头的值（如 `archive_template`）yaml MUST 加引号
- 非法值走 `frameworks/config.rs::sanitize`（warn + 回退默认）；关键 fallback（`log.level`/`archive_template`）走 `eprintln_sanitize_fallback` stderr 兜底（sanitize 在 `install_logging` 之前触发）
- **`expand_env` 递归上限 `EXPAND_ENV_MAX_DEPTH=32`** 防栈爆
- **env value 拼回 yaml 前 MUST `sanitize_env_value`** 剥换行/控制字符防结构注入
- 结构化日志字段：`feature` / `operation` / `result`
- **R1 外置**：`copy.{timezone_offset_hours, unique_name_max_attempts, archive_template, doc_archive_template}` / `exif.valid_date_time_secs` / `backend.smb.{default_user,workgroup,timeout_secs}` / `backend.adb.{server_host,server_port}` / `backend.classify.{embed_model_path,tokenizer_path,categories,score_min,max_text_bytes}` / `log.level`（`RUST_LOG` > flag > 配置）；**无消费点勿加占位**
- **不外置例外**：算法常量（EPOCH_1904 等）/ 协议字面量 / 日志维度名 / 流式哈希（`FAST_READ_SIZE`, `STREAM_CHUNK=1MiB`, `MIME_SNIFF_BYTES=256`）
- `usecases/copy/ops.rs::println!` 是 CLI 脚本可读输出**不是** R3 日志路径

### 环境变量
| 变量 | 用途 |
|---|---|
| `TIDYMEDIA_CONFIG` | 指定 config.yaml 路径 |
| `RUST_LOG` | 日志级别（优先级最高） |
| `CARGO_PROFILE_RELEASE_OPT_LEVEL` | 快速编译验证切 opt=0（justfile `OPT` 单点） |
| `TIDYMEDIA_OCR_DET_MODEL` / `TIDYMEDIA_FACE_*` | 模型路径 + 阈值 |
| `TIDYMEDIA_CLASSIFY_{MODEL,TOKENIZER,SCORE_MIN,MAX_TEXT_BYTES}` | copy-doc 内容分类模型 + 阈值 |
| `TIDYMEDIA_DOC_ARCHIVE_TEMPLATE` | copy-doc/move-doc 默认归档模板 |
| `SMB_USER` / `SMB_PASSWORD` / `SMB_TIMEOUT_SECS` / `KRB5CCNAME` | SMB 凭据与超时（KRB5CCNAME 读入未接入） |
| `ANDROID_HOME` / `ANDROID_NDK_HOME` | 交叉编译 |

## Android（feature `android-app`）
- uniffi 0.31 proc-macro 模式：`uniffi::setup_scaffolding!()` + `#[uniffi::export]` / `#[derive(uniffi::Record)]` / `#[derive(uniffi::Error)]`
- **`#[derive(uniffi::Error)]` 字段名不能叫 `message`**（与 `kotlin.Exception.message` 撞名）；用 `text`/`detail`
- `[lib] crate-type = ["rlib"]`；cdylib **不写死 Cargo.toml**（Windows 与 bin PDB 同名冲突 cargo#6313），交叉编译 `cargo rustc --crate-type cdylib` 按需覆盖
- 交叉编译：`cargo ndk -t aarch64-linux-android -p 30 --output-dir mobile/android/app/src/main/jniLibs rustc --lib --crate-type cdylib --release --features android-app`
- Kotlin 绑定：`uniffi-bindgen generate --library <libtidymedia.so> --language kotlin --out-dir <dir>`
- **`tidy_with` 单一入口返 `CommandResult` enum**（CLI 丢弃、mobile match 取 report）；MUST NOT 新增 `*_report()` 专用包装
- **mobile FFI 嵌套集合 MUST `Vec<Record>`** 禁 `paths.join(",")` CSV（路径含逗号被 Kotlin `split(",")` 拆错）
- 工具链：JDK 25 (Temurin) + Gradle 9.1 + AGP 8.10 + Kotlin 2.0.21 + NDK r26d + SDK android-35（AGP 8.7 不支持 JDK 25）

## 项目 Gotcha
- **测试 Windows 可移植**：Unix-only API MUST `#[cfg(unix)]` gate、import 放函数内；路径前缀比较用 `file_info::full_path` 规范化，MUST NOT 手动 `canonicalize()`
- **跨平台 path 段断言用 `std::path::Path::ends_with`** 优于 `s.ends_with("a/b") || s.ends_with(r"a\b")`
- **`cargo nextest run` 默认 fail-fast** 让分散平台失败被截断 → 调试跨平台 MUST `--no-fail-fast`
- **`Cargo.toml` 业务关键 dep MUST caret 锁主版本**（`sha2 = "0.11"` 非 `"*"`）
- **`tokenizers` crate `default-features = false` MUST 加 `features = ["fancy-regex"]`**（正则后端二选一缺省即编译错；fancy-regex 纯 Rust 免 C 依赖）
- **本地 `[patch.crates-io]` 验证**：`cp -r ~/.cargo/registry/src/<idx>/<crate>-<ver>/ <vendor>` + patch + path；Cargo.lock 中 patched crate 无 `source =`/`checksum =` 即生效
- **远端 `RemoteClient::read` 整文件入堆**（已知限制）：smb2 `read_file`/adb_client API 限制，大视频在 Android 有 OOM 风险
- **远端 `mkdir_p` 是真递归**（`remote.rs::mkdir_recursive`）自底向上 stat；`FakeRemoteClient::mkdir` 不校验父目录（单层 vs 递归差异 fake 测不出）
- **存在性查询 Err MUST 传播**，MUST NOT `unwrap_or(false)`（吞 Err 让后续 `open_write` truncate 覆盖）；`LocalBackend::exists` MUST `try_exists()?` 不 `.exists()`
- **mkdir/exists best-effort 缓存 MUST `contains + insert`** 非 `insert-returns-bool`（try_op 失败时 set 未污染 → 后续不跳过重试）
- **测试 shim MUST `#[cfg(test)]` gate**：`Info::from` / `Index::visit_dir` / `Exif::from_path_with_offset` / `fake_remote`
- **`#[cfg(test)]` 标在方法/import 上不标在 `impl Foo {}` 块**（同块生产方法会一起 gate 掉）
- **`chrono::TimeDelta::seconds(i64)` 会 panic**（secs > `i64::MAX/1000`）；外部 timestamp MUST `try_seconds()?` + `checked_add_signed`；**`milliseconds(i64)` 不 panic**
- **`epoch_to_candidate(u64)` 三段守护** `try_from + try_seconds + checked_add_signed`（u64 > i64::MAX 折负绕过 1904/future filter → 1969/12 桶）
- **重复组容器 MUST `Vec<DuplicateGroup>`** 禁 `BTreeMap<size, _>`（同 size 不同 content 互相覆盖）
- **含 secret 字段的 struct MUST 手写 `Debug`**（derive 让 `debug!(?t)` 明文写日志）
- **外部 JSON schema 同字段历史 String/Number 两态** → `serde_json::Value` + 二态 match（Google Takeout `photoTakenTime.timestamp`）
- **`ReportSink` 用 `enum Report<'a>` + `write(&Report<'_>)`** 单方法收敛（对象安全 + 新增变体不强制升级 impl）
- **`Info::cloned_at(loc, backend)` 复用 src hash 入 `output_index`**：dst 重新 `Info::open` 浪费且 NFS/防病毒抢占下让 dst 已写未入索引 → 重复副本
- **跨设备 `fs::rename` fallback 是 copy+remove 非原子**：半态 Err 文案 MUST 含 `copied ... but cannot remove source`
- **`use rayon::prelude::*` 违反 P0 §5**：最小 import = `use rayon::iter::{IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator}`
- **同盘 move fast-path**：`opts.remove && src.scheme()=="local" && output.scheme()=="local"` 走 `Backend::rename`（OS 内核唯一权威源判同卷，MUST NOT Rust 层重复判）
- **大文件 OOM fixture 用 random-noise PNG**（≥ 1500×1500，纯色压缩太小无效）
- **dry-run log / 路径聚合用 Bun**：临时 `cat > x.ts <<'TSEOF' ... TSEOF` + `bun x.ts`；独立脚本 MUST `bun <script>`
  - **Windows 输出陷阱**：Python stdout 默认 CRLF MUST `sys.stdout.reconfigure(newline='\n')`；GBK 中文乱码 → 写 UTF-8 文件再读；Python `re` alternation leftmost-first（长 token 放前）
- **`tracing::*!` message 含 `{name}` 被当 named placeholder** → 转义 `{{name}}` 或改措辞避 `{`
- **`MediaWriter::finish` flush MUST `?` 传播不 `.ok()`**（BufWriter 包装后 disk-full 在 finish 阶段暴露）
- **`BufWriter::into_inner` 三阶段闭合**：`bw.flush()?; let inner = bw.into_inner().map_err(IntoInnerError::into_error)?; inner.finish()?`（Drop 默认 flush 忽略 Err）
- **find 删除脚本 Python 输出**：`#!/usr/bin/env python3` + `import os` + `os.remove("...")` 待删 / `# os.remove("...")` 保护；路径转义 `\\` → `\\\\`, `"` → `\\"`；SURVIVOR 分支保护首份加 `# SURVIVOR (no copy under output)` 标记防用户删光副本
- **tract `outputs[i].cast_to::<f32>()` 三步走**：`let cow = ...?; let view = cow.view(); let slice = view.as_slice::<f32>().map_err(io::Error::other)?;`（多输出模型每 tensor 独立持 binding 防 borrowed dropped）
- **clippy `field_reassign_with_default`**：MUST 用 struct literal `T { field: v, ..T::default() }`；全字段显式给值时禁 `..Default::default()`
- **f32 if-else 字面量类型推导**：`let x: f32 = if ...` 或 `1.0_f32`
- **`#![allow(dead_code)]` 占位**：新增算法子模块（`cull/{face_align,identity_cluster,face_scoring}`）独立 commit 未消费时 module-level allow 合规；接入 commit 同时删；`#[expect(dead_code)]` 会 `unfulfilled_lint_expectation`
- **新增整模块先做 lib.rs re-export + dispatch 装配再写主体**（避免整模块 dead_code 数十条噪声）
- **office 子模块 stub 签名用 `&mut dyn MediaReader`** 而非 `Box<dyn>`（后续 zip/cfb/read_to_end 兼容零改动）
- **sed 批量加反引号陷阱**：`sed 's/PID_FOO/\`PID_FOO\`/g'` 会改 `const PID_FOO:` 定义本身；sed 后 MUST grep 校验
- **批量给多行函数调用加参数**：正则 `\([^()]*\)` 只支持一层嵌套必漏改（`&local_loc(out.path())` 两层即失配），MUST 用 Python 括号深度计数定位闭括号；改后 `git diff` 核对改动数
- **`serde_json::to_vec_pretty` 对 f32 NaN/Inf 输出 `null` 非 Err**：含 `ScoreBreakdown` 的 manifest 直接 `.expect("internal: infallible Serialize")` 无需 match Err arm
- **`max_by` 选最大 f32 项 MUST 把 NaN 视为 -∞**：抽 `total_cmp_nan_as_neg_inf(a,b)`（`partial_cmp().unwrap_or(Equal)` 让 NaN 被选中 best）
- **f32→u32 像素坐标 clamp 三态**：NaN→0 / 负→0 / +Inf→limit（`!is_finite → 0` 一锅端让 +Inf 归零 → 占位框）
- **clippy `neg_cmp_op_on_partial_ord`**：f32 上 `!(a >= b)` 拒绝；NaN-safe 改 `a.is_nan() || a < b`
- **`OffsetDateTime::from(SystemTime)` 内含 panic**（`.expect("Duration doesn't fit")`）：生产 MUST `duration_since(UNIX_EPOCH)?` + `i64::try_from` + `from_unix_timestamp` 三段守护
- **跨 backend helper（`stream_copy`）判 same-scheme** MUST `Arc::ptr_eq` 非 `scheme() == scheme()`：`FakeBackend("local")` vs `LocalBackend` 都声明 `"local"` 但存储互不可见
- **`/code-review` 假阳性识别**：recall 模式过报；「dead variant」先 `rg` 查引用；「fake silent Ok」核对真实后端 POSIX 行为；「stub map_error」按未来接入价值评估
