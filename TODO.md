# TODO

## cull 子命令（4 模型印证流水线 + 性能优化 + 严格覆盖率 100% 已落地）

### P0 e2e 真跑验证
- [x] 4 个 ONNX 模型已落地 `models/`（git-lfs 跟踪）：
  - `scrfd_10g_bnkps.onnx`（antelopev2 默认变体；500M 无开源 onnx）
  - `mobilefacenet.onnx`（foamliu/MobileFaceNet pt→onnx，128 维而非 512 维）
  - `face_mesh_192x192.onnx`（PINTO_model_zoo 静态 192×192）
  - `eyestate_yolov8.onnx`（YOLOv8 检测头，非 MobileNetV3 softmax；decode 已重写）
- [x] tract_eyestate.rs decode 重写（YOLOv8 `[1, 6, 8400]` anchor max closed conf）
- [x] tract_mobilefacenet.rs EMBED_DIM 改 128（trait `FaceEmbedder` 接口同步 `[f32; 128]`）
- [ ] 真跑 dry-run + Netron 校对：`cargo run --release -- cull /tmp/test-photos -o /tmp/culled --dry-run --report /tmp/culled/cull-report.json`（profile.release opt-level=0 让 ONNX 推理慢，e2e 真跑前应临时切 opt=3 或部署后再验）
- [ ] 按真跑反馈微调（如 YOLOv8 EyeState closed_index 是 0 或 1、SCRFD-10G 三 stride 输出顺序）

### P1 完整 4 模型印证流水线
- [x] 新增 `src/usecases/cull/face_align.rs`：ArcFace 5 点 4-DOF 相似变换 → 112×112 RGB（法方程 + Gauss-Jordan）
- [x] 新增 `src/usecases/cull/identity_cluster.rs`：跨图 embedding 余弦相似度 Union-Find 聚类
- [x] 新增 `src/usecases/cull/face_scoring.rs`：EAR 几何 + EyeState 双印证闭眼惩罚 + 嘴角上扬微笑加分综合评分
- [x] `run.rs::pick_best` 重写：4 模型印证流水线全部接通
  - SCRFD bbox + 5 keypoints → face_align → 112×112 → MobileFaceNet 128 维 embedding
  - identity_cluster 跨图余弦聚类（debug! 输出簇数，留 per-identity 策略接入点）
  - bbox 裁原图 → FaceMesh 468 点 → EAR 几何（左右眼平均）
  - SCRFD 5 点眼坐标 crop → EyeState 闭眼概率（EAR 或 EyeState 任一命中即判闭眼）
  - face_scoring::score_image 出完整 ScoreBreakdown；选 `breakdown.total` 最高者为 best
  - 单脸 face_align/facenet Err 整脸丢弃；facemesh/eyestate Err 退化为空 mesh / 0 概率

### P2 性能 / 鲁棒性优化
- [x] pHash 升级 Average Hash → DCT pHash（32×32 → 8×8 低频中位数 64-bit；JPEG 重压缩 Hamming ≤ 12）
- [x] `pick_best` 重读字节优化：ScannedFile 增 `raw_bytes: Arc<Vec<u8>>` + `decoded: Arc<RgbImage>`，scan 一次读字节 + 一次 decode 后 SCRFD/face_align/facemesh/eyestate 共享（消除二次 IO 与二次 decode）
- [x] 大图 OOM 防护：scan 阶段 entry.size 超 `backend.face.max_image_bytes`（默认 50 MiB）→ record_failure 计入 failed 不读字节
- [ ] 远端 backend 真机验证（**「首版 Local-only」标注已过期**：生产路径已全抽象，`factory.for_location` + `Backend::walk`/`read_all`，零 LocalBackend 硬连）：剩余缺口 = 远端整文件入堆 OOM 画像（靠 F9）+ SMB/MTP/ADB 实机验证

### P3 测试与覆盖率
- [x] 严格覆盖率 4 项 100%（region/function/line/branch）：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch --ignore-filename-regex='(adapters/backend/[a-z]+_real\.rs|adapters/(ocr|face)/tract_[a-z]+\.rs)$' --all-features`（`usecases/cull/run.rs` + 5 个 `tract_*_real.rs` 已回收：前者 `filter_blurry`/`analyze_image` 私有微测试补齐，后者 5 个 `load_runnable` 加 `#[coverage(off)]`；ignore-regex 只留 backend `_real.rs`（大文件+真环境）与 tract 主体（subprocess phantom）；口径 = 聚合 lcov DA/BRDA 真值 100%，summary 剩 monomorphize instance phantom 属已知）

### P4 Android FFI 集成（首版未包）
- [ ] `src/frameworks/mobile.rs` 新增 `tidy_cull(sources, output, dry_run) -> MobileCullReport`（uniffi Record）（`CommandResult::Cull`/`dispatch_cull`/`CullReport` 已就绪，照 `MobileFindReport` 平移）
- [ ] `MobileCullReport` / `MobileGroupReport` / `MobileCulledEntry` 嵌套 Record（`GroupReport`→`culled: Vec<CulledEntry>`，禁 CSV 逗号拼接）
- [ ] mobile/android 应用层 UI（缩略图视图浏览 group 目录人工对比）
- 验收：FFI 走 `tidy_with` + `expect_cull`（禁 `*_report()` 包装）；每 export 顶部 `install_config_loader()`；`mobile_tests.rs` 仿 `find_duplicates_*` 系列；`groups` 透传体积（`ScoreBreakdown` 4×f32）评估上限

## copy / move 性能优化（review 提出 14 项；已落 3，封板 5，待 6）

### 已完成
- [x] **F4 mkdir_p 缓存**：`ops.rs::do_copy` 加 `mkdir_cache: &mut HashSet<Location>`，同 `{year}/{month}` 桶下 N-1 次 mkdir_recursive RTT 收敛到 1 次；`FakeBackend` 加原子 `mkdir_p_calls` 计数辅证。测试 `mkdir_cache_skips_repeated_mkdir_for_same_target_dir` PASS。
- [x] **F8 独立 rayon I/O 池**：新 `entities/threadpool.rs::install_io`（CPU×4 clamp [8,64]），`file_index.rs::visit_location` / `parse_exif` / `enrich_candidates` 三处 `par_iter` 包入；远端 IO 阻塞不再吃 CPU 池让 pHash / EXIF 解析饿死。4 个 threadpool 单测 PASS。
- [x] **F14 stream_copy 1 MiB buffer**：`BufReader`/`BufWriter::with_capacity(1<<20)` + 显式 `flush → into_inner → finish` 三阶段闭合（disk-full 在 finish 阶段显式抛而非 Drop swallow）。`std::io::copy` 8 KiB → 1 MiB，本地 syscall 数 ÷128。

### 已封板（无独立改进空间）
- **F2 SMB/ADB 拆锁**：pavao libsmbclient C 句柄 + adb sync TCP socket 协议级串行，Mutex 是协议要求；真改进需连接池。
- **F7 secure_hash 复用**：`Index::exists` bucket 命中才算 secure，预算所有反而反优化；与 F3 绑定才有价值。
- **F11 sidecar 单 stat**：`read_to_string` 现 `stat + read`，NotFound 短路到 1 RTT 即最优。
- **F12 dry_run 跳过 mkdir_p**：`do_copy` 既有 `if opts.dry_run { return Ok(true); }` 已实施。
- **F13 dry_run 跳过 hash**：空 `output_index` 下 `secure_hash` 不触发；`fast_hash` 必算（visit 阶段每文件 4 KiB 唯一 ID）。

### 待新会话（按 ROI 排序）

#### ~~F1 `run_copy_loop` 并行化~~（已完成 2026-07-04）
- [x] `Index` 内 `HashMap` 改 `DashMap<Utf8PathBuf, Info>` + `DashMap<u64, Mutex<HashSet<Utf8PathBuf>>>`（内嵌 `DashSet` 有同 shard 递归死锁风险，issue #79）；`Index::exists` / `add` / `remove_under_prefix` 全 `&self`
- [x] `do_copy` 改接 `&Index` + `&DashSet<Location> mkdir_cache`；`run_copy_loop` 用 `install_io(|| groups.par_iter().reduce(CopyDelta::merge))`
- [x] Phase A 按 `fast_hash` 分桶 + 桶内 `full_path` 字典序 → 同 hash 组 winner 确定性；桶间并行、桶内串行
- [x] 测试 `run_copy_loop_parallel_correctness_deterministic`（10 轮独立跑断言 (10 copied, 5 ignored, 0 failed) 稳定）+ `run_copy_loop_dedup_produces_single_winner_deterministically` + `add_concurrent_lands_all_entries_and_groups_by_fast_hash` + `add_concurrent_same_fast_hash_bucket_holds_all_paths` + `remove_under_prefix_concurrent_with_readers_does_not_deadlock`
- [x] 严格覆盖率 4 项与 main HEAD 基线完全对齐（21 region miss 全在 main 已有 miss 文件：naming.rs / crop.rs / group_writer.rs / uri.rs / config.rs，未引入新 miss）

#### F3 合并 Info::open + Exif::open + sniff_mime（~3h）
- 新 `Info::open_full(loc, backend, with_exif: bool)`：单次 `open_read` → `fast_hash_stream` + 头 64 KiB head buffer + `sniff_mime`（前 256 B）+ `secure_hash_stream`（同流 sha-512）
- `Exif::open` 改 `Exif::from_head(head: &[u8], reader, loc, mime, offset)` 接预 buffered head + tail reader 续读容器深处
- `Index::visit_location` 改用 `open_full`，`parse_exif` 不再独立 `open_read`
- 远端单文件 3 次完整下载 → 1 次
- 测试 `single_open_read_computes_all_hashes_and_exif`

#### F10 unique_name + sidecar 共享 dir-listing cache（~1.5h，需 Backend trait 扩）
- Backend trait 加 `list_dir(loc) -> io::Result<Vec<String>>` 浅 list（单层）；（⚠ 实为 4 个源码点覆盖 7 面：local / remote 泛型 / fake / fake_remote，非「5 个 impl」）
- `generate_unique_name` 持 `dir_cache: &mut HashMap<Location, HashSet<String>>`：首次访问目录调 `list_dir` 入 cache；候选名查本地 set 替 `backend.exists`
- `sidecar::discover_with_backend` 共享同 cache：探测前查 set 不存在则跳过 `read_to_string`
- 高冲突 K+1 次 stat RTT → 1 次 list；无 sidecar 文件远端 2 stat RTT → 0
- 验收：⚠ 命名防串层（`list_dir` 与 F5 `RemoteClient::list_dir_with_size` 同名不同层，建议 `Backend::list_dir_shallow` vs `RemoteClient::list_with_size`）；三类测试（OK / client Err 注入 / 非自家 scheme `InvalidInput`）+ `unique_name_dir_list_cache_hits_skips_backend_exists` 计数断言

#### F5 RemoteClient::list_dir_with_size 默认 + SMB override（~1h）
- `RemoteClient` trait 加默认方法 `list_dir_with_size(target) -> Vec<(name, kind, size)>`；默认走 `list_dir + 逐个 stat`（向后兼容）
- SMB pavao 是否支持原生 stat-with-listing 待查（若否走并发 stat 池；⚠ 已核 `smb_real.rs` 每 file 二次 stat 是内部逻辑，收益 = pavao 原生能力，**SPIKE 先行**）
- ADB `ADBListItem` 已带 size，直接 wrap 免补 stat（**对 ADB 是 no-op**，仅省 SMB）
- 同步 `fake.rs` / `fake_remote.rs`；命名防与 F10 `Backend::list_dir` 串层
- 1000 文件扁平目录 N+1 RTT → 1 RTT
- 验收：fake 注入 stat 计数断言 `list_with_size_returns_size_without_extra_stat`

#### F6 walk channel pipeline（~1.5h）
- `RemoteBackend::walk` 改用 `crossbeam_channel::bounded(1024)` + `std::thread::spawn` worker：`walk_recursive` 在独立线程 send entry，立即吐给消费者
- `Backend::walk` 返 `Box<dyn Iterator<Item = io::Result<Entry>>>` 接 `channel.into_iter()`，Drop 时关 channel + join thread
- `visit_location` 的 `par_iter` 可在树未完时已开始处理首批 entry，scan 与 process 流水线重叠
- ⚠ **ROI 低（2026-08-25 核验）**：现有消费者（`visit_location` 串行收 locs 再 par_iter、cull `scan_source`）都是「先走完 walk 再处理」，channel 化不改变消费者结构，「流水线重叠」假设当前不成立；需连带改消费者才有效，建议降级或与 F3 顺带

#### F9 RemoteClient::read 改 streaming Reader（~2.5h，trait 破坏 + 线程桥接）
- `RemoteClient::read` 返 `Box<dyn Read + Send + 'static>`
- SMB 走线程 + `std::sync::mpsc` + `Cursor` 桥接 `SmbFile` 非 `'static` 借用（新 `adapters/backend/remote_pipe.rs::PipeReader`）
- ADB pull 回调式 API 同款桥接（pull 写 pipe writer，consumer 读 reader）
- `RemoteBackend::open_read` 直接转发，移除 `read_to_end` 整文件入堆
- `stream_copy` 真正流水线（边下边写），单文件峰值内存从 2× size 降到 buffer
- ⚠ **必须 F3 后做**：F3 依赖 open_read 多次 seek（head buffer + 容器深读续读），F9 改 forward-only pipe 与其语义冲突；⚠ 「2×→buffer」对 copy 路径言过其实（写侧 BufferedWriter 仍整文件缓冲），真收益在 sniff/EXIF/小块读与 Android FFI 内存
- 验收：`stream_copy` 峰值内存断言（fake cap RSS 或缓冲大小注入）+ 全量 grep `RemoteClient::read` 调用点后再动签名

#### F15 远端 Backend override 原生原子 rename（~2h，消除 default 非原子 fallback）
- SMB pavao：SMB2 `SET_INFO` `FileRenameInformation`（1 RTT 服务端原子）
- ADB：`adb shell mv src dst`（同 fs 原子）
- MTP libmtp：`SendObjectPropList` Rename op
- 每 backend override `Backend::rename` 走原生路径，非 default `copy_file + remove_file`（读整文件到本地 + 重传 + 断电半态）
- 配套 override `supports_native_rename_to` 从 default `false` → same-scheme + same-host `true`，让 `copy/ops::do_copy` fast-path 分支命中
- 测试：注 fake client 让 native rename 返 Err，caller 走非原子 fallback 分支
- move SMB→SMB 10K 文件 3 RTT ÷ 1 RTT = 67% wall time 削减

#### F16 远端 Backend `copy_file` server-side copy（~2h，同 backend 归档零字节回客户端）
- 远端 `copy_file` 现是 `client.read(src) + client.write(dst)` 两次全量 RTT + 全字节回客户端
- SMB pavao：SMB2 `FSCTL_SRV_COPYCHUNK`（服务器端复制，零字节回客户端）
- ADB：`adb shell cp /sdcard/A /sdcard/B`（同设备内 shell 复制）
- MTP libmtp：`SendObjectPropList` Copy op（若协议支持）
- `RemoteClient` trait 加 `try_server_side_copy(src, dst) -> io::Result<Option<u64>>`：不支持返 Ok(None) 让 caller fallback；协议 Err 上抛
- `RemoteBackend::copy_file` 先试 server-side，None 时才 read+write
- SMB→SMB 10K × 500 MiB 视频归档：10 TB 网络流量 → 0（服务端本地复制）

#### F17 边界硬限与配置外置（~1h，YAGNI 补边界）
- `remote::walk_recursive` 加 MAX_DEPTH=256 硬限：远端深备份树（Time Machine / rsync `--link-dest`）超限中断该子树 + record walker_error，防堆内存耗尽。⚠ 已是迭代式 stack + visited（无递归栈崩，`remote.rs:229-279`）；stack 当前不携带 depth，加限需改栈元素结构，工作量较原估重；`MAX_DEPTH` 更近算法常量（CLAUDE.md「不外置例外」）
- `MAX_REMOTE_WRITE_BUFFER = 2 << 30` 外置到 `backend.remote.write_buffer_limit_bytes`，Android FFI 场景可调小到 512 MiB fail-fast，桌面可关（`u64::MAX`）；P0 §13 数值常量 MUST 从配置读取。有真实消费点（`remote.rs:100,:470`），走「新增配置字段」同步检查点全链（config.rs + config.yaml + sanitize + defaults 测试 + rg 验证消费点）
- （已完成）`system_time_to_offsetdatetime` 三段守护单元测试：`naming.rs:132-145` 实现 + `naming.rs:179-194` 测试已覆盖；原拟 `UNIX_EPOCH + Duration::from_secs(u64::MAX)` fixture 构造不出（`checked_add` 超范围返 None）

### 落地建议
1. ~~先做 F1~~（已完成 2026-07-04）
2. F3 + F10 一起做（共享 cache 基础设施）
3. F5 / F6 / F9 / F15 / F16 一并改 RemoteClient trait（避免多次破坏；F15+F16 复用同一批 backend override 骨架）
4. 落地后跑 Linux + `--all-features` `cargo +nightly llvm-cov --branch` 严格 4 项 100% 验证

## 媒体识别缺口（tidy-verify 实证）

- [ ] **Panasonic RW2（RAW）未识别为媒体**（2026-08-09 tidy-verify `D:\Users\Public\Pictures\2023` 实证 218 个，`12/高一元旦晚会` Panasonic DMC-GF6）：magic `II U \0`（0x49 0x49 0x55 0x00，第三字节非 TIFF 0x2A），`infer` 不识 → `mime_from_ext` 无 .rw2 → 空 mime → `passes_type_filter` 跳过；应纳入媒体全集。⚠ **不能直调 `parse_tiff`**（`tiff_ifd.rs` 硬校验 0x002A 必 None），须参数化 magic 入口内部走 `parse_ifds` 复用（AVI `strd` 同款）；走「新增容器 EXIF 自解析」检查点（`mime_from_ext` 增映射 + `types.rs::from_reader` 新分支 + `image_rw2.rs` + `gen_rw2.py` fixture）。⚠ 远端 `open_read` 整文件入堆（RAW ≥20 MB）OOM 面色，fixture 近端 only。落地前可用 `exiftool -FileModifyDate<DateTimeOriginal` + `--include-non-media` 兜底（本轮已如此归档）

## media_time 文件名解析缺口（tidy-verify 实证）

- [x] `2010-01-10 11-01-12.mp4`（`YYYY-MM-DD HH-MM-SS` 空格分隔日期时间）→ `try_generic_dashed` + `FilenameDashedDateTime` 已覆盖（`filename_tests.rs:289-323`）
- [x] `VID_20180205_110003.mp4`（标准 Android 相机 `VID_YYYYMMDD_HHMMSS`）→ `try_camera_or_phone` VID_ 前缀已覆盖（`filename_tests.rs:41-50`）
- [ ] **括号 / QQ 导出时戳两形态纳入 `entities/media_time/filename.rs`**（2026-08-09 tidy-verify `D:\Users\Public\Pictures\2021` 实证，16 mp4 + 6 jpg 已用 exiftool 按文件名写回容器/EXIF 时间侧修复数据）：
  - `IMG_6489(20210611-174530)(1).jpg`：括号内 `yyyyMMdd-HHmmss`
  - `QQ图片20210428220203.jpg`：`QQ图片` 前缀 + 14 位 `yyyyMMddHHmmss`（连续无分隔）
  - ⚠ **黑名单归属待实证**（2026-08-25 三视角分析）：内嵌时戳若来自原图 EXIF 拍摄时间（与下载 mtime 不同源），「多数派互证恒真」前提不成立，可能**不应**进 `is_majority_filename_vote` 黑名单；实现前取真实样本核对归属再定
  - 实现约束：新 matcher MUST 插 `try_loose_yyyymmdd`（链末）之前（否则被 loose 先吞 8 位日期降为 `FilenameBareYyyymmdd`）；`(N)` 序号后缀先剥；新 Source 进 P2 + 按实证结论定黑名单
- 落地时按「新增 `media_time` 候选」同步检查点走 `priority.rs` + `filename.rs` + fixture

## media_time 容器解析缺口（tidy-verify 实证）

- [ ] **MP4 QuickTime Keys/UserData `DateTimeOriginal` 解析**：iPhone 编辑导出副本场景下，`mvhd` CreateDate 是导出时戳而非拍摄时间，真实拍摄时间存于 Keys/UserData 的 `DateTimeOriginal`（带时区字符串，如 `2020:07:25 20:40:10+08:00`）；nom-exif 只读 `mvhd`，tidymedia 误归导出月。实证：`D:\Users\Public\Pictures\2020\08\IMG_1511~1525.mp4` ×8（2026-08-04 tidy-verify 发现，已用 exiftool 改写 mvhd 侧修复数据）。⚠ **先实证再立项**：诊断「nom-exif 只读 mvhd」与 CLAUDE.md「iPhone `com.apple.quicktime.creationdate` 已被 fork 合并进 `TrackInfoTag::CreateDate`」矛盾，需真机编辑导出片实测 nom-exif 返回值确认缺口真伪。⚠ **「priority 高于 QuickTimeCreationDate」无实现路径**：`resolve` 排序 `(priority asc, utc asc)` 同 P0 仅靠早 utc 决胜，需决策「Keys 存在时抑制 mvhd 候选」或「mvhd 降档到空悬 `QuickTimeCreateDate`(P1)」。时区：`+08:00` 经 `DateTime<FixedOffset>::timestamp()` 直接转 UTC，走 `push_epoch(secs, None)` 通道（不按配置 offset 解释）。建议实体自解析（riff/png/m2ts 模式）而非再叠 nom-exif fork 分支；BMFF 分支条件用 MIME 族非字面量。落地时按「新增容器 EXIF 自解析」+「新增 `media_time` 候选」双检查点走 `priority.rs` + `video.rs` + fixture（含 `decision.source` 断言）
- [ ] **XMP 老 `xap:` 前缀 + element 形式漏读**：Photoshop CS2 时代 XMP 用 `xap:` 命名空间前缀（`xap:CreateDate`/`xap:ModifyDate`）且可写作 element 形式 `<xap:CreateDate>2008-10-31T09:15:01+08:00</xap:CreateDate>`；`entities/xmp.rs` 只认 `xmp:` 前缀 attribute 形式 → `populate_image_xmp_fallback_if_empty` 静默 miss 落 mtime。实证：`C:\Pictures\2008\10\m2b4dmzt.jpg`（2026-08-23 tidy-verify 发现，exiftool 归一显示为 `[XMP] CreateDate` 掩盖了前缀差异，已用 exiftool 写 AllDates 侧修复数据）。落地：日期抽取增 `xap:` attribute + element 两形态；⚠ element 是标签体文本，与 `find_attr_rfc3339` 的 attribute 路径形态不同，需独立抽取函数；`xap:ModifyDate` 是 re-save 旁证不进候选；测试用 `exif_xmp_tests.rs` 内联 head 字节（exiftool 生不出真 `xap:` 前缀，需手拼）
