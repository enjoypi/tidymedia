@~/.claude/CLAUDE.md

@~/.claude/rust.md

# tidymedia 开发上下文

按拍摄或创建时间去重整理媒体与文档的 Windows/macOS CLI，支持本地与 SMB 来源，采用 Clean Architecture。

## 全局原则

1. 文档与沟通使用简体中文和全角标点，表达精简、无歧义、便于人工维护；文件名用英文。
2. 给用户的选项带数字编号，第 1 项为推荐项或 All。
3. 批量改动使用脚本或工具，所用脚本及新增通用自动化脚本使用 Bun + TypeScript；既有平台包装和模型准备脚本可保留原技术栈。批量替换后核对匹配数量和结果。
4. 遵守 SOLID、DRY、YAGNI：职责单一、面向接口、逻辑只保留一份；优先复用仓内代码、stdlib、平台能力和已有依赖。
5. 代码文件含测试不得超过 512 行，达到 400 行时预防性拆至不超过 300 行；函数不得超过 64 行。`CLAUDE.md` 必须少于 256 行。
6. 不写注释或 docstring，删除已有注释。
7. 存储与日志时间使用 UTC；面向人展示时标注时区并支持 `TZ`。
8. 新配置使用 YAML。用户可调默认值和业务字符串从配置读取；算法、协议和固定 IO 边界常量可留在代码。配置必须有真实消费点。
9. secret 只从环境变量加载，不设默认值，不写入 YAML 或日志。
10. 输出 debug 级结构化日志，包含定位请求所需字段；逐文件事件用 `debug!`，用户汇总提示用一条 `warn!`。
11. 测试先行，先写失败测试再实现；先满足 branch 100%，再满足 region 100%。测试与业务代码分文件。
12. 不自行新建 git branch 或 worktree；未经用户明确要求不 commit、不 push，不 amend 或改写已有提交。同一主题的未提交改动合并为一个新 commit，不同主题分开；message 单行、少于 64 个 UTF-8 字符、无署名，格式为 `[TASK-ID ]type: 中文业务描述`，type 仅取 build/chore/ci/docs/feat/fix/perf/refactor/revert/style/test。TASK-ID 由用户提供或取最近 git log，没有则留空。
13. 可复用规则写入对应作用域的 `CLAUDE.md` 并去重；一次性规则不保留。`TODO.md` 仅用于开放任务，完成即删除对应事项。
14. Plan 使用 subagent 从至少三个角度推敲，包含批判角度。定稿后把计划移到项目目录，完成后删除。
15. 新建文档须先征得用户同意。README 只写使用方法；`docs/` 只写非技术业务需求；AI 规则写入对应 `CLAUDE.md`；skill 保留其可执行结构。

## 常用命令

- 日常构建、测试和门禁统一使用 justfile，不直接调用 cargo；没有对应 recipe 的专项操作除外。
- `just build`：nightly + dev 构建。
- `just test`：全量测试；`just test <名称>` 可过滤。
- `just fmt`、`just lint`：格式化和 lint。
- `just cov`：覆盖率门禁，已包含全量测试，不重复执行 `just test`。
- `COV_ZERO_IGNORE=1 just cov`：零忽略审计，用于核对全部生产源码缺口。
- `just gate`：完整门禁，具体步骤以 justfile 为准。
- `just build-release`：发布、真模型和性能运行使用 release opt=3；仅需快速编译检查时可用 `just OPT=0 build-release`，不得用该产物评估性能。

## 数据安全底线

- dry-run 不写归档媒体、不删源；显式指定的报告文件除外。存在性查询失败必须传播，不能按“不存在”处理。
- 本地写入不得覆盖已有目标；移动类操作只能在写入成功或确认内容重复后删源。
- 正式移动、覆盖时间信息和不可逆删除必须获得用户明确同意。
- 不因性能优化削弱判重、过滤、重叠保护、半态错误和失败报告。

## 规则索引

- 修改 Rust 架构、算法、CLI、backend 或配置：阅读 `src/CLAUDE.md`。
- 修改测试、fixture 或覆盖率断言：阅读 `tests/CLAUDE.md`。
- 修改 Bun 脚本、模型准备或性能采集：阅读 `scripts/CLAUDE.md`。
- 修改 justfile、MSVC 环境或覆盖率工具：阅读 `tools/CLAUDE.md`。
- 用户能力和非技术验收要求见 `docs/requirements.md`；用户操作以 `README.md` 为准。
- 归档前对账、时间写回及经确认后的正式移动使用 `/tidy-verify`，流程以 `.claude/skills/tidy-verify/SKILL.md` 为准。
