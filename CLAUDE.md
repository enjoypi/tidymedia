@~/.claude/CLAUDE.md

# tidymedia 开发上下文

按拍摄或创建时间去重整理媒体与文档的 Windows/macOS CLI，支持本地与 SMB 来源，采用 Clean Architecture。

## 全局优先原则

1. 文档与沟通精简无歧义、人工可维护
2. 使用简体中文（全角标点），不用繁体中文、日文、韩文。文件名用英文
3. 向用户提问尽量细，前因后果说清楚，一项一项问。选项带数字编号，第 1 项为推荐
4. 脚本用 Bun + TypeScript，不用 Perl、Python、Bash
5. SOLID：一个类/模块/函数只负责一项功能；针对接口编程；拆分胖接口；用 Interface/Trait 而非继承
6. DRY：同一逻辑只保留一份，抽取为函数/模块
7. YAGNI：只写当前需要的功能；依次优先复用仓内代码、stdlib、平台原生能力、已装依赖，都不行才写能跑通的最小实现
8. 代码文件（含测试）不超过 512 行、函数不超过 64 行，超限即拆分
9. 不写注释或 docstring，删除已有注释
10. 存储与日志时间用数字时间戳+UTC，并且支持 TZ 环境变量
11. 配置文件用 yaml；默认值、数值常量、业务字符串常量从配置读取
12. secrets 通过 sops 管理，yaml 中不设默认值，写法 `db_password: "${DB_PASSWORD}"`
13. 输出 debug 级结构化日志（session、feature、user、action、request、response 等）供 AI 分析，日志中禁止 secrets
14. 测试先行：先写失败的测试再实现；覆盖率先保证 branch 100% 再保证 region 100%；Web 前端用 CDP+DOM 进行测试，禁止截图
15. 测试与业务代码分文件
16. 不自行新建 git branch 或 worktree
17. git commit message 单行、少于 128 个 UTF-8 字符、不含 Co-Authored-By 等签名，格式 [TASK-ID ]type: 业务描述，type ∈ build/chore/ci/docs/feat/fix/perf/refactor/revert/style/test，TASK-ID 由用户提供或取自最近 git log，没有则留空
18. 可复用的规则、原则、坑尽快写进对应领域的 CLAUDE.md（与既有规则去重合并，单个 CLAUDE.md 少于 256 行），不记入 TODO.md 或 MEMORY；一次性规则丢弃
19. TODO.md 是任务清单，任务完成即删除
20. Plan 定稿后把 ~/.claude/plans/<name>.md 移到当前项目目录，无需确认。Plan 完成后删除
21. 新建文档（.md .rst .adoc .txt 等）先征得用户同意
22. 后端提供的 API 必须是严格的 HTTP Method + RESTful，返回值必须严格遵守 RESTful 规范
23. 前端必须使用后端生成的 OpenAPI 描述进行通信和强类型检查，禁止自行猜测
24. 前端必须能独立部署于 CDN 上
25. 能并发的尽量并发。必须考虑线程间同步方案是否最优
26. 执行后台长任务时可以创建个定时器来监控任务是否正常运行没有卡住

## 常用命令

- 日常构建、测试和门禁统一使用 justfile，不直接调用 cargo；没有对应 recipe 的专项操作除外。
- 工具链：`rust-toolchain.toml` 固定 rolling nightly（含 rustfmt、clippy、llvm-tools），禁止日期版；dev 与覆盖率走 nightly，release 产物走 stable。
- `just build`：nightly + dev 构建。
- `just check`：nightly 全目标类型检查，与 `lint`/`build`/`test` 共享依赖产物。
- `just test`：全量测试；`just test <名称>` 可过滤。
- `just fmt`、`just lint`：格式化和 lint。
- `just cov`：覆盖率门禁，已包含全量测试，不重复执行 `just test`。
- `COV_ZERO_IGNORE=1 just cov`：零忽略审计，用于核对全部生产源码缺口。
- `just gate`：完整门禁，具体步骤以 justfile 为准。
- `just build-release`：发布、真模型和性能运行使用 stable + release opt=3；仅需快速编译检查时可用 `just OPT=0 build-release`，不得用该产物评估性能。

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
