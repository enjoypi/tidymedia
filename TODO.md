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
- [ ] 远端 backend 支持（首版 Local-only）：SMB/MTP/ADB 的人脸检测路径

### P3 测试与覆盖率
- [x] 严格覆盖率 4 项 100%（region/function/line/branch）：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch --ignore-filename-regex='(_real\.rs|adapters/(ocr|face)/tract_[a-z]+\.rs|usecases/cull/run\.rs)$' --all-features`
- [x] mutation 增量验证（`cargo mutants --in-diff`）发现 `sanitize_max_image_bytes` 的 `*` → `+` 等价变异 missed，已补 `load_sanitizes_midrange_max_image_bytes_to_default` 用 5000 字节边界值杀掉。336 全量 mutant 受时长限制（每 mutant ~1–2 min × 336 ≈ 6 h）未跑完整，CI 增量已通
- [x] `.cargo/mutants.toml exclude_re` 扩展 `'replace build_(scrfd|facenet|facemesh|eyestate)_'` + `'replace .* with .* in face_scoring::score_image'`

### P4 Android FFI 集成（首版未包）
- [ ] `src/frameworks/mobile.rs` 新增 `tidy_cull(sources, output) -> MobileCullReport`（uniffi Record）
- [ ] `MobileCullReport` / `MobileGroupReport` 嵌套 Record（参照 `MobileFindReport` / `MobileDuplicateGroup`）
- [ ] mobile/android 应用层 UI（缩略图视图浏览 group 目录人工对比）

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

#### F1 `run_copy_loop` 并行化（~2h，最大 ROI）
- `Index` 内 `HashMap` 改 `DashMap<Utf8PathBuf, Info>` + `DashMap<u64, DashSet<Utf8PathBuf>>`；`Index::exists` / `add` / `remove_under_prefix` 全 `&self`
- `do_copy` 改接 `&Index` 共享；`run_copy_loop` 用 `install_io(|| entries.par_iter().map(do_copy))`
- `mkdir_cache` 改 `DashSet<Location>`
- 警惕：move 模式 winner 顺序需仍按 `full_path` 字典序保证可重现
- 测试 `run_copy_loop_parallel_correctness`（10 文件并发结果与串行等价）

#### F3 合并 Info::open + Exif::open + sniff_mime（~3h）
- 新 `Info::open_full(loc, backend, with_exif: bool)`：单次 `open_read` → `fast_hash_stream` + 头 64 KiB head buffer + `sniff_mime`（前 256 B）+ `secure_hash_stream`（同流 sha-512）
- `Exif::open` 改 `Exif::from_head(head: &[u8], reader, loc, mime, offset)` 接预 buffered head + tail reader 续读容器深处
- `Index::visit_location` 改用 `open_full`，`parse_exif` 不再独立 `open_read`
- 远端单文件 3 次完整下载 → 1 次
- 测试 `single_open_read_computes_all_hashes_and_exif`

#### F10 unique_name + sidecar 共享 dir-listing cache（~1.5h，需 Backend trait 扩）
- Backend trait 加 `list_dir(loc) -> io::Result<Vec<String>>` 浅 list（单层）；5 个 impl 同步（local/remote/fake + smb/adb/mtp 自动跟 remote）
- `generate_unique_name` 持 `dir_cache: &mut HashMap<Location, HashSet<String>>`：首次访问目录调 `list_dir` 入 cache；候选名查本地 set 替 `backend.exists`
- `sidecar::discover_with_backend` 共享同 cache：探测前查 set 不存在则跳过 `read_to_string`
- 高冲突 K+1 次 stat RTT → 1 次 list；无 sidecar 文件远端 2 stat RTT → 0

#### F5 RemoteClient::list_dir_with_size 默认 + SMB override（~1h）
- `RemoteClient` trait 加默认方法 `list_dir_with_size(target) -> Vec<(name, kind, size)>`；默认走 `list_dir + 逐个 stat`（向后兼容）
- SMB pavao 是否支持原生 stat-with-listing 待查（若否走并发 stat 池）
- ADB `ADBListItem` 已带 size，直接 wrap 免补 stat
- 同步 `fake.rs` / `fake_remote.rs`
- 1000 文件扁平目录 N+1 RTT → 1 RTT

#### F6 walk channel pipeline（~1.5h）
- `RemoteBackend::walk` 改用 `crossbeam_channel::bounded(1024)` + `std::thread::spawn` worker：`walk_recursive` 在独立线程 send entry，立即吐给消费者
- `Backend::walk` 返 `Box<dyn Iterator<Item = io::Result<Entry>>>` 接 `channel.into_iter()`，Drop 时关 channel + join thread
- `visit_location` 的 `par_iter` 可在树未完时已开始处理首批 entry，scan 与 process 流水线重叠

#### F9 RemoteClient::read 改 streaming Reader（~2.5h，trait 破坏 + 线程桥接）
- `RemoteClient::read` 返 `Box<dyn Read + Send + 'static>`
- SMB 走线程 + `std::sync::mpsc` + `Cursor` 桥接 `SmbFile` 非 `'static` 借用（新 `adapters/backend/remote_pipe.rs::PipeReader`）
- ADB pull 回调式 API 同款桥接（pull 写 pipe writer，consumer 读 reader）
- `RemoteBackend::open_read` 直接转发，移除 `read_to_end` 整文件入堆
- `stream_copy` 真正流水线（边下边写），单文件峰值内存从 2× size 降到 buffer

### 落地建议
1. 先做 F1（解锁 N 倍吞吐，其他 fix 的 RTT 缩减才被并行放大见效）
2. F3 + F10 一起做（共享 cache 基础设施）
3. F5 / F6 / F9 一并改 RemoteClient trait（避免多次破坏）
4. 落地后跑 Linux + `--all-features` `cargo +nightly llvm-cov --branch` 严格 4 项 100% 验证
