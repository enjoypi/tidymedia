# 测试与覆盖率规则

## 门禁与组织

- 日常命令使用根 `CLAUDE.md` 的 just 入口。
- 覆盖率门禁为 Windows 与 macOS 双平台各自 `just cov`，branch、region、function、line 均为 100%；先消除 branch miss，再处理 region。
- 跑测器固定为 nextest，其「每测试独立进程」是测试套件的成立前提：`set_env_var`/`remove_env_var` 的 SAFETY 注释、`usecases::config` 的 `once` + `Box::leak` 进程级单例都依赖它。不得为提速把 cov 跑测切成 `cargo test`（实测 lib 174〜220 个失败，详见 `tools/CLAUDE.md`）。
- 覆盖率排除只认 `tools/cov.ts` 的逐文件精确清单，维护规则见 `tools/CLAUDE.md`；禁止按目录或 `tract_*`、`_real`、`_io` 等后缀排除，禁止 `coverage(off)` 及其 `cfg_attr` 形式。
- 判断存量 miss 归属时，用相同 filter 对照未修改基线；平台固有缺口不得通过降低门禁掩盖。
- 单元测试外置为 `#[cfg(test)] #[path = "X_yyy_tests.rs"] mod yyy_tests;`。多份测试共享 helper 放 `<mod>_tests_common.rs`，在主模块顶层挂载，不嵌套 `mod tests {}` 后误用相对 `#[path]`。
- 集成测试的 `tests/<name>.rs` 是 root binary，子目录文件用 root 的 `#[path]` 装配，不作为独立 binary。
- 测试 shim 必须 `#[cfg(test)]` gate。

## 覆盖率处理

- tracing fixture 通过 ctor 安装全局 subscriber，只开启 `tidymedia*` target，让宏字段在覆盖运行中真实求值。新增集成测试根必须挂载。
- 定位 miss：子行 region 用复用 profdata 的 `llvm-cov report --text`；branch 查 BRDA 未执行记录；function 用 lcov FNDA，跨 instance 全零才视为真 miss。多实例缺口不是排除理由。profraw 由门禁用后即删，事后凭据只剩 `target/llvm-cov-target/*.profdata` 与 `target/coverage.lcov`。
- 定长转换、整数上限等结构不可达的 `?` 改为无 Err 分支的 map、unwrap_or 或索引 helper；能由 helper 直接返回最终 Result 时，不在 caller 留下不可达 `?`。
- 拆分联合 assert，并先计算 panic 文案中的表达式，避免失败路径产生未覆盖子表达式。
- 难触发的 Err arm 优先使用 fn pointer 或 `#[cfg(test)]` shim 注入，不为覆盖率改变生产语义。
- 多个调用点共享的短路条件抽成 helper；私有纯函数用微测试覆盖每个短路分支和 NaN/边界输入。
- 平台分支使用 `#[cfg(target_os = "...")]` 常量，避免 `cfg!()` 让当前平台产生不可达 instrument 分支。
- office bin instance 由 `OFFICE_FIXTURES` 集中驱动；office 入口和复杂业务函数的全部业务分支由 lib 单元测试覆盖。
- 超大常量守卫通过纯 helper 接受测试值，不在测试中构造巨型资源。
- 泛型函数的 region 覆盖按单个实例计：必须有一个实例走遍全部分支，不能靠多个实例拼凑；测试用的泛型实例（如 `serde_json::Value`、测试闭包）要同时覆盖成功和失败路径。
- 内存额度相关测试用 `usecases::memory::with_retained` 或小容量 `Budget` 注入，额度只作用于当前线程的扫描阶段。

## Fixture 与 Fake

- `tests/data/` 的 fixture 由 `bun tests/fixtures/gen.ts` 生成（需要 exiftool 与 ffmpeg），产物入库，测试运行期不依赖这两个工具；doc/xls/ppt 由 `gen_cfb_fixtures` 测试生成。
- `tests/data/` 的时间测试必须用 `std::fs::FileTimes` + `File::set_times` 固定 mtime（lib 侧走 `entities::test_common::set_mtime`，集成侧走各 root 的 `set_mtime` helper），不得依赖 checkout 后的 mtime；共享 PNG helper 使用 `FIXED_MEDIA_MTIME`。钉时间需以写权限打开文件，负 Unix 秒表示 1970 年前。
- MP4 需要“无创建时间”语义时使用 MKV fixture；未显式写 creation_time 的 MP4 可能得到 1904 时间。
- FakeBackend 默认 mtime 为 UNIX_EPOCH，会走 P4 并在默认时区落入 `1970/01`；测试需要其他桶时显式设置时间。
- DCT pHash 使用至少 256×256 的随机噪声 JPEG；亮度变化断言 Hamming 在既定阈值内。纯色图和 gradient 不能替代随机噪声 fixture。
- 大文件内存 fixture 使用足够大的随机噪声 PNG，不使用高压缩纯色图。
- FakeBackend 的 reader error 与 operation error 文案和触发阶段不同；测试 sniff 成功后 read 失败时，选择对应注入点。
- FakeBackend.copy_file 按 src loc 注入错误；唯一名耗尽测试用 add_file 填满候选，不另写生产 wrapper。
- `fake_remote::list` 只返回直属子项；递归 walk 测试必须组合 add_dir 与 add_file。
- FakeBackend.walk 对不存在 root 返回空，LocalBackend 会记录 walker error；扫描完整性测试必须使用真实后端。
- `supports_native_rename_to` 的成功分支使用真实 LocalBackend 和 tempfile 验证。
- RemoteClient fake 共享状态并支持按 op/path 注入错误；mkdir fake 不验证父目录，真递归语义不能由 fake 证明。

## 平台与错误注入

- Unix-only API 和 import 用 `#[cfg(unix)]` gate；跨平台路径段断言优先 `Path::ends_with`，不要手写两种分隔符后缀。
- Windows 同卷测试使用 subst/junction，相关 nextest group 串行执行，并用 RAII 清理 subst。
- 权限错误通过 chmod 0 父目录触发；chmod 0 文件本身不会让 metadata 在所有平台失败。
- 测试环境变量通过模块内单点 helper 设置和清理，避免在测试中散落 unsafe env 调用。
- 配置 OnceLock 测试使用独立 YAML 和 `TIDYMEDIA_CONFIG`，不依赖全局加载顺序。
- fast-path 测试固定 src mtime 并断言 dst mtime；rename/copy 与流式复制的 mtime 语义不得混淆。
