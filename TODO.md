# TODO

## cull 子命令（首版骨架已落地，待 e2e 真跑验证后补完整 4 模型印证流水线）

### P0 e2e 真跑验证
- [x] 4 个 ONNX 模型已落地 `models/`（git-lfs 跟踪）：
  - `scrfd_10g_bnkps.onnx`（antelopev2 默认变体；500M 无开源 onnx）
  - `mobilefacenet.onnx`（foamliu/MobileFaceNet pt→onnx，128 维而非 512 维）
  - `face_mesh_192x192.onnx`（PINTO_model_zoo 静态 192×192）
  - `eyestate_yolov8.onnx`（YOLOv8 检测头，非 MobileNetV3 softmax；decode 已重写）
- [x] tract_eyestate.rs decode 重写（YOLOv8 `[1, 6, 8400]` anchor max closed conf）
- [x] tract_mobilefacenet.rs EMBED_DIM 改 128（trait `FaceEmbedder` 接口同步 `[f32; 128]`）
- [ ] 真跑 dry-run + Netron 校对：`cargo run --release -- cull /tmp/test-photos -o /tmp/culled --dry-run --report /tmp/culled/cull-report.json`
- [ ] 按真跑反馈微调（如 YOLOv8 EyeState closed_index 是 0 或 1、SCRFD-10G 三 stride 输出顺序）

### P1 完整 4 模型印证流水线（占位算法待实装）
- [ ] 新增 `src/usecases/cull/face_align.rs`：ArcFace 5 点 4-DOF 仿射 → 112×112 RGB
- [ ] 新增 `src/usecases/cull/identity_cluster.rs`：跨图 embedding 余弦相似度 Union-Find 聚类
- [ ] 新增 `src/usecases/cull/face_scoring.rs`：综合评分（清晰度 - 闭眼惩罚 + 微笑加分）
- [ ] `run.rs::pick_best` 重写：
  - SCRFD 出 bbox + 5 keypoints
  - face_align 出 112×112 对齐人脸
  - FaceNet 出 512 维 embedding → cross-image 身份聚类
  - bbox 裁原图 → FaceMesh 出 468 点 → EAR 几何
  - keypoints 裁眼部 → EyeState 出闭眼概率（双印证）
  - face_scoring 综合评分（per-identity 选代表 face，避免大合影一闭眼小孩拉低全图）

### P2 性能 / 鲁棒性优化
- [ ] pHash 升级 Average Hash → DCT pHash（HDR bracket 序列更鲁棒）
- [ ] `pick_best` 重读字节优化：scan 阶段缓存 + Arc<Vec<u8>> 共享给 SCRFD（减一次 IO）
- [ ] 大图 OOM 防护：scan 阶段 entry.size 超阈值（如 50 MB）skip
- [ ] 远端 backend 支持（首版 Local-only）：SMB/MTP/ADB 的人脸检测路径

### P3 测试与覆盖率
- [ ] 跑严格覆盖率：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch --ignore-filename-regex='(_real\.rs|adapters/(ocr|face)/tract_[a-z]+\.rs|usecases/cull/run\.rs)$' --all-features`，确认 4 项 100%
- [ ] 跑 mutation：`cargo mutants --in-diff <(git diff main)`，目标 0 missed
- [x] `.cargo/mutants.toml exclude_re` 扩展 `'replace build_(scrfd|facenet|facemesh|eyestate)_'`（与 `build_smb/mtp/adb_backend` 同套路）

### P4 Android FFI 集成（首版未包）
- [ ] `src/frameworks/mobile.rs` 新增 `tidy_cull(sources, output) -> MobileCullReport`（uniffi Record）
- [ ] `MobileCullReport` / `MobileGroupReport` 嵌套 Record（参照 `MobileFindReport` / `MobileDuplicateGroup`）
- [ ] mobile/android 应用层 UI（缩略图视图浏览 group 目录人工对比）

## 其他
（暂无）
