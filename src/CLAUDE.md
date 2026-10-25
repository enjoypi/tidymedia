# Rust 开发规则

## 架构与文件组织

- 依赖方向为 `frameworks/` → `adapters/` → `usecases/` → `entities/`；内层不得依赖外层。验证：`rg "use crate::adapters|use crate::frameworks" src/entities/ src/usecases/` 不得出现实现依赖。
- 入口保持薄：`bin/tidymedia.rs` 只安装 config loader 并调用 `run_cli(env::args_os())`；`lib.rs` 只包含模块声明、re-export、loader 安装和 uniffi setup。
- `usecases/` 只依赖 `entities/`。跨层服务通过内层 `OnceLock` loader、fn pointer 或 trait 注入，不用 re-export 桥接。
- 推理 Output Port 放在对应 `usecases/<feature>/`，基础设施 Port 放在 `entities/backend/`；具体 impl 放 `adapters/`，选择 impl 的 factory 放 `frameworks/`。
- `tidy()` 在部分失败时返回 Err 以提供非零退出码；`tidy_with()` 直接返回 `CommandResult`，不重复检查。
- 生产目录使用 `foo/mod.rs` 声明与 re-export，子模块不得命名为 `core`。测试需要访问的内部项用 `#[cfg(test)]` 私有 re-export 或 `pub(super)`。
- 新模块先完成 `lib.rs` re-export 和 dispatch 装配再实现主体；仅在未消费模块的独立阶段允许 `#![allow(dead_code)]`，接入时删除。
- `#[cfg(test)]` 标在方法或 import 上，不标在同时含生产代码的 `impl` 块上。

## 配置与日志

- 配置结构在 `usecases/config.rs`，加载与 sanitize 在 `frameworks/config.rs`，示例和默认值以 `config.yaml` 为准，不在文档复制配置清单。
- `install_config_loader()` 必须在 bin 入口、每个 FFI export 和需要切换配置的集成测试中最先调用；lib 单元测试使用 crate 入口安装。
- `TIDYMEDIA_CONFIG` 指定配置文件；`RUST_LOG` > `--log-level` > `log.level`。
- `expand_env` 必须限制递归深度；环境值拼回 YAML 前使用 `sanitize_env_value`。展开后以 `{` 开头的 YAML 值必须加引号。
- 非法配置经 sanitize 回退默认；日志安装前发生的关键回退同时用 stderr 提示。
- 结构化日志包含 `feature`、`operation`、`result` 等维度。`feature` 常量统一从 `usecases::report` 获取，包括 verify，不另建副本。
- CLI 的逐文件 stdout 是脚本输出，不替代结构化日志。

## URI、路径与 Backend

- URI 支持本地路径、`local://`、`smb://[user@]host[:port]/share/path`、`adb://[serial]/abs/path` 和保留的 `mtp://device/storage/path`；MTP 当前返回 Unsupported。
- URI 字段使用 percent-encoding，不引入 `url` crate；IPv6 host 使用方括号。ADB 多设备必须提供 serial。
- SMB 用户名可由 URI 或 `SMB_USER` 提供，密码只从 `SMB_PASSWORD` 读取；Kerberos 配置读入不代表已支持。
- 新路径拼接使用 `Location::join_path(segment)`，远端目标内部也用 `/` 语义；纯本地计算才使用 OS 分隔符。
- copy/move/cull/move-text-shot 的前缀规范化统一使用 `canonical_prefix`；entry 级 output 子树判断使用 `entry_under_prefix`。路径索引 key 使用 `file_info::full_path`，不要以 canonicalize 替代字面稳定路径。
- source ⊆ output 必须拒绝；output ⊂ source 时必须剔除 output 子树。比较要考虑分隔符边界、symlink 和 Windows 大小写。
- 新增 Backend 方法时同步所有真实与 fake 实现，并覆盖成功、client Err、非本 scheme `InvalidInput`。
- Backend `walk` 必须递归产生所有文件；远端遍历使用 `visited` 防环。浅列表缺少 size 时，补充 stat 的错误必须上抛。
- `copy_file` 拒绝跨 scheme；跨 scheme 使用 `open_read`、`open_write`、流式复制和 `writer.finish`。判断同一存储实例用 `Arc::ptr_eq`，不能只比 scheme 字符串。
- Backend 能力通过 trait method 查询，不在调用方硬编码 scheme。同盘 move 由 OS rename 是否成功判定，不在 Rust 层按盘符猜测。
- 本地 `exists` 使用 `try_exists()?`；本地 `rename`、`copy_file`、`open_write` 必须不覆盖已有目标，冲突返回 `AlreadyExists`。远端没有统一的原子 no-replace 保证，调用方必须通过索引、命名和冲突错误保护目标，不得宣称与本地同等保证。
- feature 未启用时，backend `new()` 统一通过 `remote::unsupported_backend(feature)` 返回 Unsupported，不复制错误文案。
- mkdir/exists best-effort 缓存使用 `contains + insert`，避免失败操作污染缓存。
- 远端 mkdir 使用真递归；client 原始调用统一经对应 `map_error`，操作日志统一经 `map_and_log`。NotFound/AlreadyExists 用 debug，其余错误用 warn。
- 错误文案重映射先保留已正确分类的 kind，再以 ASCII 小写匹配 NotFound 和 PermissionDenied；不得吞掉其他错误。
- ADB 删除使用不带 `-f` 的 `rm`，shell exit 为 None 必须视为失败。
- 远端 `RemoteClient::read` 当前可能整文件入堆；修改读取、EXIF 或图像路径时必须考虑大文件内存，不得把拆目录当作根本修复。
- 远端整文件写入必须设上限并使错误可见；flush/finish 错误必须传播。

## media_time 与容器解析

- 优先级：P0 为原始拍摄或文档创建时间，P1 为数字化/容器写入时间，P2 为文件名启发式，P3 为 XMP sidecar/Google Takeout，P4 为文件系统时间。具体 Source 以 `entities/media_time/priority.rs` 为准。
- 多数派推翻 P0 只接受 `Validity::Valid` 候选；下载时戳类文件名不得参与投票，新增此类 Source 必须加入 `is_majority_filename_vote` 黑名单。
- ModifyDate 只作旁证：与 filename 票互证时否决多数派推翻并记录冲突，不成为独立候选。
- naive EXIF 和文件名时间按配置时区解释；QuickTime/MKV 等 UTC 时间先转 UTC，再按配置时区确定归档桶。EXIF naive 的桶可直接看原字符串年月，UTC 容器时间不可套用此规则。
- `epoch_to_candidate(0)` 表示未填；合法的文件系统 UNIX_EPOCH 由独立转换路径处理。外部 timestamp 必须经整数转换、受限时间构造和 checked arithmetic 防溢出。
- 1904 时间按当天窗口过滤；未来时间和 1995 年前低置信度候选按现有 filter 规则处理。
- 冲突 `diff_secs = chosen.utc - other_utc`；override 分支 chosen 为新 winner，other 为原 P0 best。
- sidecar 属于 `adapters/sidecar.rs`，通过依赖倒置注入 copy；backend-aware 路径规则不得假设所有来源都是 Local。
- 容器时间解析复用 `tiff_ifd::parse_tiff` 或 `parse_ifds`：AVI、PNG eXIf、JPEG APP1 fallback、M2TS SEI、RW2 和老 QuickTime 按各自现有入口接入，不另建平行解析体系。
- `tiff_ifd::scan_ifd` 保持 lenient；TIFF ASCII count ≤ 4 读取 inline 值，count > 4 才按 offset。
- 非媒体解析统一使用 `Exif::open_filtered` 与 `Index::parse_exif` 的 parse_non_media/doc_only 三态；生产代码不使用测试 shim `Exif::open`。落盘 `passes_type_filter` 与解析短路必须同口径。
- JPEG 主解析失败、EXIF 双 0 或容器 TIFF 缺失时，按现有三态进入 XMP fallback；统一调用 `populate_image_xmp_fallback_if_empty`。XMP 同时覆盖 `xmp:`、`xap:` 的 attribute/element 形式及 `photoshop:DateCreated`。
- ZIP 与 CFB 文档嗅探结果必须经扩展名 MIME 重映射；完整容器分类不能依赖短 sniff 窗口。
- nom-exif tracing 默认压为 `nom_exif=error`；GPS 子 IFD 用 `Exif::iter()` 按 tag code 查找。

## 复制、移动、报告与推理

- 类型过滤必须先于重复删源；dry-run 不写归档媒体、不删源（显式报告文件除外），但要预填 hash 并把预计目标加入 output index，保持与真跑相同的命名决策。
- 目标名通过 `Index::try_reserve_target` 原子占位，Windows 按路径组件小写折叠；`AlreadyExists` 时尝试下一候选名。
- `generate_unique_name` 依次使用 output index、权威索引判定和 backend exists；修改 walk/VisitStats 语义时同步复核权威判定。
- 目标已存在时按 size、双侧内容判等区分 Duplicate、Fresh、Exhausted；只有 Duplicate 才能删源并计入去重。
- `Info::cloned_at` 复用源 hash 和 category 更新 output index，不重新打开目标文件。
- copy 成功但删源失败使用 `partial_move_error`；目标写入失败使用 `output_write_error`，输出拒绝后熔断剩余写入。检测必须使用专用 downcast/helper，不靠文案 contains。
- 无 Index 的移动用例必须检查 sources/output 重叠、sources 两两重叠和空文件名，单文件失败不得 panic。
- OCR/图像等 bytes 用例先读取 sniff 窗口，命中目标类型后再读完整内容；预分配容量必须处理 `u64` 到 `usize` 的转换。
- copy-doc/move-doc 仅在文档模式且模板含 `{category}` 时装配分类器；分类在 EXIF/MIME 解析后进行，并经 `Info.category` 传入模板。分类阈值以下使用 `uncategorized`，路径段必须 sanitize。
- 含 `errors` 的 Report 使用统一软上限和截断标记，rayon merge 传播截断状态；`failed` 不受上限影响。
- `scanned` 在 walk 循环中按触达数累计，不能用成功项数量代替。耗时统一使用 `elapsed_ms`；新增字段同步所有对应 Report、FFI 镜像、fixture 和 JSON 断言，不写死 Report 数量。
- `ReportSink` 使用 `enum Report<'a>` 和单一 `write(&Report<'_>)` 方法保持对象安全；新增变体不应强制所有 sink 增加专用方法。copy/move 的部分失败文案按实际操作分流并双向测试。
- cull 的 `culled[i].score` 与 best 综合 total 同口径；比较 f32 时把 NaN 视为负无穷。
- tract 静态 face 图使用 `into_optimized().into_runnable()`；动态 H/W 的 OCR DBNet 使用 `into_typed().into_runnable()`；BERT symbolic 维先 `into_typed()`，绑定 batch=1、sequence=256，再优化。tokenizer 输出手动 pad/truncate 到 sequence。
- tract runnable 已由 Arc 持有，不重复包装。MobileFaceNet embedding 为 128 维，FaceMesh 输入 192×192，EyeState 输入 640×640 letterbox，decode 使用其 `[1,6,8400]` 输出契约；SCRFD 是检测器，不输出身份 embedding。
- 老 QuickTime 使用现有 nom-exif patch，并只用 `BoxHeader::parse` 解析 header；M2TS/AVCHD 使用现有 video PID 与 H.264 EP strip 规则，不把 container body 一次性装入解析 buffer。
- 推理 metadata 与输入按值透传；letterbox 不得 `.min(1.0)`。face embedding 以 `identity_cluster::EMBED_DIM` 为权威并严格检查长度；当前仍有独立常量或 `[f32; 128]` 字面类型，修改维度前必须先收敛为单点。
- tract 输出 tensor 的 cow、view、slice 必须保持有效生命周期；多输出分别持有 binding。
- uniffi Error 字段使用 `text` 或 `detail`，不得使用 `message`；mobile 嵌套集合使用 `Vec<Record>`。FFI 统一消费 `tidy_with` 的 `CommandResult` 并单向映射为 FFI Record，不新增绕过该入口的并行结果通道。

## 同步检查点

- 新增 Location/backend scheme：URI FromStr、cfg-gated factory、Unsupported fallback、Backend impl、dispatch、README URI 说明同步修改。
- 新增配置字段：结构体、config.yaml、sanitize、默认值测试和真实消费点同步修改；secret 同步环境示例与忽略规则。
- 新增 CLI flag：dispatch 透传，并为每个相关子命令覆盖 Some/None；e2e 必须包含 `run_cli(["tidymedia", ...])` 字符串形式。
- 新增子命令：Commands、CommandResult、tidy/tidy_with、dispatch、usecase、Report/feature、report sink、lib re-export 和 README 同步修改。
- 新增 media_time Source/Priority：来源定义、解析、resolve/decision、多数派资格、fixture 和冲突断言同步修改。
- 新增归档占位符：render、PLACEHOLDERS、validate_archive_template 和 README 同步修改；用户值进入路径前必须 sanitize。
- 新增媒体容器：实体解析、EXIF 路由、MIME、XMP fallback、fixture 生成和测试同步修改。
- 新增 office 容器：parse/extract_text、MIME 与双路由、office fixtures、e2e 归档和扫描共用 helper 同步修改；stub 使用 `&mut dyn MediaReader`，便于 zip/cfb/read_to_end 复用。
- 新增 factory：内层 trait、frameworks Default factory、dispatch 注入、lib re-export 及所有调用点同步修改。
- 修改 FaceEmbedder 维度：trait、真实 impl、fake、单测、集成测试和 `EMBED_DIM` 使用点同步修改。
- 修改 FindReport 或其他 FFI 镜像 schema：领域 Report、mobile Record、report sink fixture 和 JSON 断言同步修改。

## Rust 与依赖陷阱

- `chrono::TimeDelta::seconds` 可能 panic，外部秒数使用 `try_seconds` 和 `checked_add_signed`；`OffsetDateTime::from(SystemTime)` 不得直接用于外部时间，必须先做 duration 和整数范围检查。
- 重复组使用 `Vec<DuplicateGroup>`，不得用 size 作为唯一 map key。
- 含 secret 的 struct 手写 Debug。外部 JSON 同一字段可能为 String 或 Number 时使用 `serde_json::Value` 分态处理。
- 含 f32 的 JSON 序列化会把 NaN/Inf 写为 null；业务上需要拒绝或保留时必须显式处理，不依赖序列化报错。
- f32 比较避免 `!(a >= b)` 和会把 NaN 选为 best 的 `partial_cmp().unwrap_or(Equal)`；像素坐标分别处理 NaN、负值和正无穷。
- `MediaWriter::finish` 的 flush 错误必须传播。BufWriter 使用 `into_inner().map_err(IntoInnerError::into_error)` 后再 finish，不额外制造不可达分支。
- 避免 `field_reassign_with_default`：使用 struct literal 和结构体更新语法；全字段显式给值时不追加 `..Default::default()`。if-else 的 f32 字面量需要明确类型。
- rayon 只导入所需 iterator traits，不使用 `rayon::prelude::*`。
- 业务关键依赖使用 caret 主版本约束；tokenizers 关闭默认特性时启用 `fancy-regex` 纯 Rust 正则后端。
- 本地 crates.io patch 验证以 Cargo.lock 中 patched crate 不再含 source/checksum 为准。
- tracing message 中的字面 `{name}` 必须转义或改措辞。
