# 测试与覆盖率规则

## 门禁与组织

- 测试先行，先确认新测试失败，再实现使其通过。日常命令使用根 `CLAUDE.md` 的 just 入口；跨平台调试 nextest 使用 `--no-fail-fast`。
- 覆盖率在 Windows 与 macOS 双平台各自运行 `just cov` 或 `just gate` 验证，均为 nightly、dev，branch、region、function、line 均为 100%。先消除 branch miss，再处理 region。
- `tools/cov.sh` 是覆盖率门禁唯一实现；工具维护规则见 `tools/CLAUDE.md`，不要在测试文档复制脚本参数。
- 判断存量 miss 归属时，用相同 filter 对照未修改基线；平台固有缺口不得通过降低门禁掩盖。
- 单元测试外置为 `#[cfg(test)] #[path = "X_yyy_tests.rs"] mod yyy_tests;`。多份测试共享 helper 放 `<mod>_tests_common.rs`，在主模块顶层挂载，不嵌套 `mod tests {}` 后误用相对 `#[path]`。
- 集成测试的 `tests/<name>.rs` 是 root binary，子目录文件用 root 的 `#[path]` 装配，不作为独立 binary。
- 测试 shim 必须 `#[cfg(test)]` gate；新增集成测试根同步挂载 tracing fixture。

## 覆盖率处理

- tracing fixture 通过 ctor 安装全局 subscriber，只开启 `tidymedia*` target，让宏字段在覆盖运行中真实求值。新增测试根必须挂载。
- 覆盖率排除的完整分类以 `tools/CLAUDE.md` 为准，包含全仓 `_phantom.rs`，并不限于模型。不得为掩盖新 miss 扩大 ignore-regex，禁止使用 `coverage(off)` 或其 `cfg_attr` 形式。
- `_phantom.rs` 只承接 per-instance phantom branch；独立文件前必须确认函数本身已被测试覆盖。
- 子行 region 用复用 profdata 的 `llvm-cov report --text` 定位；branch 查 BRDA 未执行记录，function 用 lcov FNDA，跨 instance 全零才视为真 miss。
- 定长转换、整数上限等结构不可达的 `?` 改为无 Err 分支的 map、unwrap_or 或索引 helper；能由 helper 直接返回最终 Result 时，不在 caller 留下不可达 `?`。
- 拆分联合 assert，并先计算 panic 文案中的表达式，避免失败路径产生未覆盖子表达式。
- 难触发的 Err arm 优先使用 fn pointer 或 `#[cfg(test)]` shim 注入，不为覆盖率改变生产语义。
- 多个调用点共享的短路条件抽成 helper；私有纯函数用微测试覆盖每个短路分支和 NaN/边界输入。
- 平台分支使用 `#[cfg(target_os = "...")]` 常量，避免 `cfg!()` 让当前平台产生不可达 instrument 分支。
- office 入口和复杂业务函数按现有约定排除 subprocess 覆盖，但 lib 单元测试必须覆盖全部业务分支；office fixtures 集中在 `OFFICE_FIXTURES` 驱动 bin instance。
- 超大常量守卫通过纯 helper 接受测试值，不在测试中构造巨型资源。

## Fixture 与 Fake

- `tests/data/` 的时间测试必须用 `filetime::set_file_mtime` 固定；共享 PNG helper 使用 `FIXED_MEDIA_MTIME`，不得依赖 checkout 后的 mtime。
- MP4 需要“无创建时间”语义时使用 MKV fixture；未显式写 creation_time 的 MP4 可能得到 1904 时间。
- FakeBackend 默认 mtime 为 UNIX_EPOCH，会走 P4 并在默认时区落入 `1970/01`；测试需要其他桶时显式设置时间。
- DCT pHash 使用至少 256×256 的随机噪声 JPEG；亮度变化断言 Hamming 在既定阈值内。纯色图和 gradient 不能替代随机噪声 fixture。
- 清晰度阈值使用高方差图案；同色图不能提供有效 laplacian 方差。
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
- `epoch_to_candidate(0)` 与合法 fs UNIX_EPOCH 是不同语义，分别测试，不合并断言。
