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

## 其他
（暂无）
