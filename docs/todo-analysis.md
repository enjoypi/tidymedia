# TODO.md 三视角深度分析（2026-08-25）

> 方法：3 个只读 analysis subagent 并行，分别从 ① 落点与可行性核验、② 依赖关系与排期、③ 设计一致性与风险 三个角度分析 `TODO.md`。结论高度互证，并相互修正了多处 TODO 固有假设。本文供后续排期与实现直接引用，证据均含 `文件:行号`。

## 1. 总览

| TODO 区块 | 三视角结论 | 动作 |
|---|---|---|
| cull P0/P2/P3 | P3 覆盖率已 100%；P0 真跑待实机；**P2「Local-only」标注已过期**（生产路径已全抽象，零 LocalBackend 硬连，`cull/run.rs:109`） | P0 真跑；P2 改标注 |
| 性能 F3/F5/F6/F9/F10/F15/F16/F17 | F3+F10 高 ROI；F6 ROI 低可降级；F15/F16 的 SMB 侧可行性存疑待 spike；F9 必须 F3 后；F17 一子项已过时 | 按修正后排期 |
| RW2 媒体识别 | 真缺口；**TODO「直接复用 parse_tiff」方案失败**（magic `IIU\0` 即 0x0055 ≠ TIFF 0x002A） | 参数化 magic 入口 |
| media_time 文件名 | **三种命名已实现两种**（`VID_`、空格 dashed）；真缺口仅「括号 / QQ 导出」两形态 | 重写条目 + 待实证黑名单 |
| MP4 QuickTime Keys/UserData | 真缺口存疑：诊断与 CLAUDE.md（fork 已合并 `com.apple.quicktime.creationdate`）矛盾；「priority 高于」无实现路径 | 先实证再立项 |
| XMP `xap:` 前缀 + element | 真缺口，改动封闭在 `xmp.rs` + `exif_xmp_tests.rs`（内联 head） | 约 1h |

## 2. TODO 过时 / 错误条目（3 处 plus 2 处修正）

| 条目 | 现状与证据 |
|---|---|
| 文件名「三种未解析命名」 | 两种已实现：`2010-01-10 11-01-12.mp4` → `try_generic_dashed` + `FilenameDashedDateTime`（`filename.rs:107-124`，测试 `filename_tests.rs:289-323`）；`VID_20180205_110003.mp4` → `try_camera_or_phone` 的 VID_ 前缀（`filename.rs:147-150`，测试 `filename_tests.rs:41-50`）。真缺口仅剩括号时戳与 QQ 14 位导出 |
| F17「system_time 三段守护测试」 | 已实现（`naming.rs:132-145` + 测试 `naming.rs:179-194`）；且 TODO 所述 `UNIX_EPOCH + Duration::from_secs(u64::MAX)` fixture **构造不出来**（`SystemTime::checked_add` 超范围返 None） |
| cull P2「首版 Local-only」 | 过期：`factory.for_location(source)` + `Backend::walk`/`read_all`，生产路径零 LocalBackend 硬连（`cull/run.rs:109`）。剩余缺口 = 远端整文件入堆 OOM 画像（靠 F9）+ 实机验证 |
| RW2「直接复用 parse_tiff」 | magic 第三字节不符，直接调用必 None（见 §3.1） |
| `QuickTimeCreateDate`(P1) | 空悬变体：全库仅 `priority.rs` 定义 + 测试引用、无生产生产者 —— 可承接 MP4 Keys 落地时「mvhd 降档」方案 |

## 3. 真缺口清单

### 3.1 Panasonic RW2 媒体识别（约 2h，实证 `D:\Users\Public\Pictures\2023\12\高一元旦晚会` ×218）
- **链路现状**：`passes_type_filter`（`copy/ops.rs:165,179-188`）→ `Info::is_media` → `is_media_mime`（`types.rs:255-260`，仅 `image/`|`video/` 前缀）；MIME 来源 `sniff_mime`（`mime.rs:29-41`，infer 不识 RW2）→ `mime_from_ext` 兜底（`mime.rs:106-138`，**无 .rw2 分支**）→ 空 mime → skip。
- **可行性**：仿 `image_png` 先例（`png.rs:41` → `tiff_ifd` → `image_png.rs:23` → `types.rs::from_reader` 分流 `types.rs:160`）。MIME 加 `rw2 → image/tiff` 或专属 `image/x-panasonic-rw2` 常量；`image/tiff` 即可过 `is_media_mime`，落盘过滤自动同口径。
- **⚠️ 实现前 MUST**：magic `II U\0`（0x49 0x49 0x55 0x00）第三字节 0x55，**不能直调 `parse_tiff`**（`tiff_ifd.rs:57-59` 硬校验 0x002A）；需参数化 magic 入口内部走 `parse_ifds`（`tiff_ifd.rs:67`，与 AVI `strd` 同款）。确认 IFD0 偏移与 DTO 是否在 ExifIFD 链内（`tiff_ifd` 只扫 IFD0 + ExifIFD 一层）。
- **同步检查点**：CLAUDE.md「新增容器 EXIF 自解析」链（entities + `image_rw2.rs` + `from_reader` 新分支 + `gen_rw2.ts` fixture）。
- **⚠️ 关联风险**：RAW 单文件 ≥20 MB，远端（SMB/ADB）`open_read` 整文件入堆 → OOM 面色。fixture 用近端 only。

### 3.2 XMP 老 `xap:` 前缀 + element 形态（约 1h，实证 `C:\Pictures\2008\10\m2b4dmzt.jpg`）
- **现状**：`xmp.rs` `find_xmp_packet`（`xmp.rs:37-47`）只搜 `<x:xmpmeta>`；`parse_xmp_dates`（`xmp.rs:53-59`）键常量 `:21-22` 仅 attribute `=` 形态；`find_attr_rfc3339`（`xmp.rs:61-103`）要求 key 后紧跟引号 → element 形态 `<xap:CreateDate>…</xap:CreateDate>` 必然 miss；`xmp.rs:11-12` 注释明示「element 形态 YAGNI」。
- **可行性**：`xap:` attribute 增常量；element 形态需**独立抽取函数**（attribute 与 element 值形态不同，不能复用 `find_attr_rfc3339`）；测试用 `exif_xmp_tests.rs`（`exif_xmp_tests.rs:59-100`）内联 head 字节，**无需磁盘 fixture**（exiftool 无法生成真 `xap:` 前缀，见 TODO 实证）。
- **注意**：`xap:ModifyDate` 是 re-save 旁证不进候选，别误当候选；同步更新 `xmp.rs:11-12` 的「已知不支持」注释。

### 3.3 文件名括号 / QQ 导出时戳（真缺口，约 1.5h）
- **确认未解析的两个形态**：
  - `IMG_6489(20210611-174530)(1).jpg`：解析链逐跳 miss（`try_camera_or_phone` rest≠15、`try_loose_yyyymmdd` 锚点集含 `-`/`_`/空格/0 不含 `(`，`:72-102`）。
  - `QQ图片20210428220203.jpg`：14 位 `YYYYMMDDHHMMSS` 无分隔符，全链 miss。
- **⚠️ 黑名单归类待实证**（用户已定「先实证再定」）：括号/QQ 内嵌时戳是「原图 EXIF 拍摄时间」还是「服务器/下载时刻」直接决定进不进 `is_majority_filename_vote`（`priority.rs:74-80`）。若源于原图拍摄元数据、与本地下载 mtime 不同源，则「多数派互证恒真」前提不成立，可能**不应**黑名单化。
- **实现约束**：matcher MUST 插在 `try_loose_yyyymmdd`（链末）**之前**（否则被 loose 先吞 8 位日期降为 `FilenameBareYyyymmdd`，该 variant 不在黑名单）；`(N)` 序号后缀先剥；14 位与 13 位 ms（`try_unix_millis`，`filename.rs:272-293`）天然互斥无越界风险。新 Source 进 P2 match（`priority.rs:47-67`）。
- **同步检查点**：CLAUDE.md「新增 media_time 候选」链（priority + filename + resolve 推导 + `is_majority_filename_vote` 黑名单 + fixture）。

### 3.4 MP4 QuickTime Keys/UserData DateTimeOriginal（3-4h，**先实证**）
- **矛盾待解**：TODO 诊断「nom-exif 只读 mvhd、丢失拍摄时间」vs CLAUDE.md「iPhone `com.apple.quicktime.creationdate` 已被 fork 合并进 `TrackInfoTag::CreateDate`」——若 fork 已读 Keys/ilst，现有 `populate_video_dates`（`video.rs:66-81`）**可能已在读拍摄时间**，缺口可能不存在。实现前 MUST 拿 iPhone 编辑导出片实测 nom-exif 返回值。
- **「priority 高于 QuickTimeCreationDate」无实现路径**：`resolve.rs:40-44` 排序 `(priority asc, utc asc)`，同 P0 仅靠早 utc 决胜；需决策 ① Keys 存在时抑制 mvhd 候选，或 ② mvhd 降档到空悬 `QuickTimeCreateDate`(P1)。勿假设排序即可表达层级。
- **时区**：`+08:00` 经 `DateTime<FixedOffset>::timestamp()` 直接转 UTC epoch，走 `push_epoch(secs, src, None, false)` 通道（`media_time/mod.rs:76-82` 已有 DocumentCreated/MkvDateUtc 同款），不进 `ascii_datetime_to_epoch`（`video.rs:54-62` 只解析 naive）。
- **实现路线**：建议实体自解析（riff/png/m2ts 已验证模式）而非再叠 nom-exif fork 分支；BMFF 分支条件用「MIME 族」而非字面量（`video/quicktime` + `video/mp4` + 3GP/XAVC）。
- **同步检查点**：CLAUDE.md「新增容器 EXIF 自解析」+「新增 media_time 候选」双链。

### 3.5 cull P4 Android FFI（约 2-3h；Android UI 另 5-8h）
- **样板就绪**：`CommandResult::Cull`（`dispatch.rs:22,182-190`）/ `dispatch_cull`（`dispatch.rs:301-332`）/ `CullReport`（`cull/report.rs:8-33`）/ `Report::Cull` + `FEATURE_CULL`（`report.rs:23`）。`MobileCullReport`/`MobileGroupReport` 照 `MobileFindReport`（`mobile.rs:43-63`）平移。
- **硬约束**：走 `tidy_with` + `expect_cull`（对偶 `mobile.rs:224-241`），禁 `*_report()` 专用包装；每个 export 顶部 `install_config_loader()`（`mobile.rs:98,113,131` 既有模式）；嵌套集合 `Vec<Record>` 禁逗号拼接；字段禁 `message` 用 `text`。
- **盲点**：`groups: Vec<GroupReport>` 透传无大小 guard（含 `ScoreBreakdown` 4×f32，体积比 find 组大数倍）；`errors` 建议镜像 `TidyStats` 只透聚合计数 + status，不全量透传 `ReportError`。

## 4. 性能 F 系列修正

| 项 | 修正结论 | 关键依据 |
|---|---|---|
| F3+F10（~4.5h）| 高 ROI 组合；F10 文案「5 个 impl 同步」实为 4 个源码点覆盖 7 面（local / remote 泛型 / fake / fake_remote） | RemoteClient 实现仅 3 处（smb/adb/fake_remote），Backend 泛型覆盖 smb/adb/mtp |
| F5/F15/F16（~3-4h）| **SPIKE 先行**（0.5h 查 pavao 0.2 是否暴露 stat-with-listing / SMB2 SET_INFO rename / FSCTL_SRV_COPYCHUNK）；SMB 半很可能不可行，届时只做 ADB `shell mv`/`shell cp` + 默认 fallback | `smb_real.rs:15` 明言「pavao 0.2 暴露面有限」；F5 对 ADB 是 no-op（`adb_real.rs:144` 已带 size） |
| F6（~1.5h）| **ROI 低可降级**：消费者（`file_index.rs:383`、`cull/run.rs:329`）都是「先走完 walk 再处理」，channel 化不改变结构，「scan/process 流水线重叠」假设不成立 | |
| F9 streaming Reader（~2.5-3h）| 独立会话，**必须 F3 后**（F3 依赖 open_read 多次 seek，F9 改 forward-only pipe 冲突）；真收益在 sniff/EXIF/小块读与 Android FFI 内存，「2×→buffer」对 copy 路径言过其实（写侧 BufferedWriter 仍整缓冲） | `remote.rs:364/:400/:427` 消费点 + `fake_remote.rs:207` |
| F17（~1h）| 删 system_time 子项（已实现）；`MAX_DEPTH` 更近算法常量（CLAUDE.md「不外置例外」）；`MAX_REMOTE_WRITE_BUFFER` 外置走配置检查点（有真实消费点 `remote.rs:100,470`，合规）。`walk_recursive` 已是迭代栈（`remote.rs:229-279`）不存 depth，加 MAX_DEPTH 需改栈元素 | |

## 5. 推荐实施顺序

1. **组合 A**（同一会话，entities 实证缺口，约 4-5h）：RW2 + XMP `xap:` + 文件名括号/QQ —— 共享 exif/media_time fixture 与 coverage 基建，一次 Linux 4×100% 收敛
2. **组合 B**（~4.5h）：F3+F10（共享 `visit_location`/`parse_exif` 管线上下文，同 cache 语义）
3. **组合 C**（~3-4h）：RemoteClient SPIKE → F5/F15/F16（+可选 F6）；**F9 独立会话**（最高风险、可退回）
4. **独立项**：cull P0 e2e（需实机 + Netron + `CARGO_PROFILE_RELEASE_OPT_LEVEL=3`，不与代码改混跑）；MP4 Keys（需先实证 nom-exif 返回值）；cull P4 FFI（依赖 cull 收敛）

每次落地后 MUST Linux + `--all-features` 4 项覆盖率 100% 收敛（预算含 coverage 时间）。

## 6. 风险核对清单

- **QQ/括号黑名单实证点**：取 1-2 个真实样本，核对内嵌时戳 = 原图 EXIF DTO（拍摄）还是下载/服务器时刻，再定 `is_majority_filename_vote`。黑名单只删票、不删 `FilenameOver1Day` 冲突报告（`resolve_majority_tests.rs:229` 双语义可见差异）
- **RW2 magic**：第三字节 0x55 ≠ 0x2A，`parse_tiff` 必 None，须参数化入口复用 `parse_ifds`
- **MP4 priority**：`(priority asc, utc asc)` 排序无「严格高于」语义，两候选同 P0 靠早 utc —— 需显式抑制/降档方案
- **F5/F10 命名串层**：`list_dir` 是 Backend 方法、`list_dir_with_size` 是 RemoteClient 方法，同名不同层，实现时易张冠李戴，建议 `Backend::list_dir_shallow` vs `RemoteClient::list_with_size`
- **F16 同实例约束**：`try_server_side_copy` 仅同一 backend 实例合法，与 `stream_copy` 的 `Arc::ptr_eq` 判定（`backend/mod.rs:204`）交互，跨实例会复刻 Fake vs Local 存储穿透问题
- **远端大 RAW OOM**：`open_read` 整文件入堆（CLAUDE.md 已知限制），SMB/ADB 源 + RW2 风险面扩大，fixture 近端 only