# tidymedia

按拍摄时间或创建时间去重整理照片、视频和文档。将 `tidymedia`（Windows 为 `tidymedia.exe`）放入 PATH 后使用。

## 准备

- 本地核心处理不启动外部进程；只有选择执行 `find` 生成的 Python 脚本时才需要 Python。
- SMB 和 ADB 需要所用二进制在编译时启用 `smb-backend`、`adb-backend` 支持；默认 `just build-release` 不包含这些后端，未启用时会返回 Unsupported。
- 从 Android 设备读取前，安装 Android Platform Tools，开启 USB 调试，并执行 `adb start-server` 与 `adb devices` 确认设备已授权。多设备必须在 URI 中指定 serial。
- 使用 SMB 时，通过 `SMB_USER`、`SMB_PASSWORD` 提供凭据；也可在 URI 的 `user@` 中指定用户名。密码不要写入配置文件。
- `cull`、`move-text-shot` 和按内容分类的文档归档需要相应模型。模型路径及阈值见 `config.yaml`，也可用对应的 `TIDYMEDIA_*` 环境变量覆盖。
- MTP 来源目前不可用；Android 设备请使用 ADB URI。

## 通用用法

```text
tidymedia [--log-level <LEVEL>] <COMMAND> [OPTIONS] <SOURCES...>
```

- `SOURCES` 可包含一个或多个文件、目录或 URI；含空格的路径要加引号。
- 除 `find` 外，需要输出的命令都用 `-o <DIR>` 或 `--output <DIR>` 指定归档目录。
- `copy`、`move`、`copy-doc`、`move-doc`、`move-text-shot`、`cull` 支持 `--dry-run`，不修改源或归档媒体；显式指定 `--report` 时仍会写报告。
- 所有子命令都支持 `--report <PATH>`，将本次结果写为 JSON 报告。
- 全局参数放在子命令前，例如 `tidymedia --log-level debug copy ...`；`--dry-run` 放在子命令后。
- `find` 的脚本和 `verify` 的汇总写 stdout；普通 `copy`/`move` 的逐文件对应关系通过 debug 日志写 stderr，其他日志和错误也使用 stderr。
- 报告中的 `failed` 和 `errors` 应作为验收依据。完整参数可用 `tidymedia <COMMAND> --help` 查看。

来源格式：

```text
C:\Photos
/home/user/Photos
local:///home/user/Photos
smb://[user@]host[:port]/share/path
adb://[serial]/absolute/path
```

URI 字段中的空格、中文等字符使用 percent-encoding；IPv6 地址用方括号，例如 `smb://[::1]/share/photos`。单设备 ADB 可写 `adb:///sdcard/DCIM`，多设备则写 `adb://SERIAL/sdcard/DCIM`。

## 复制或移动媒体

```bash
tidymedia copy -o <OUT> <SOURCES...>
tidymedia copy -o <OUT> --dry-run --report <REPORT> <SOURCES...>
tidymedia move -o <OUT> <SOURCES...>
```

- 默认只处理照片和视频；其他文件会跳过。需要一并处理时加 `--include-non-media`。
- 目标库已有相同内容时，不重复归档。`copy` 保留源文件；`move` 会删除已确认重复的源文件。
- `move` 还会移动其他待归档源文件，成功后源文件不再保留。
- 目标存在同名但内容不同的文件时，会选择新名称；本地写入不覆盖已有文件。远端没有统一的原子防覆盖保证，整理期间不要让其他进程同时修改目标库。

归档模板用 `--archive-template` 覆盖，默认 `{year}/{month}/{valuable_name}`：

| 占位符 | 含义 |
|---|---|
| `{year}` `{month}` `{day}` | 归档时间的年、月、日 |
| `{make}` `{model}` | 相机厂商和型号；未知时为 `unknown` |
| `{valuable_name}` | 最靠近文件的含非 ASCII 字符目录段；没有则省略 |

示例：

```bash
tidymedia copy -o <OUT> --archive-template '{year}/{month}/{day}' <SOURCES...>
```

## 复制或移动文档

```bash
tidymedia copy-doc -o <OUT> <SOURCES...>
tidymedia move-doc -o <OUT> <SOURCES...>
```

只处理支持的文档，包括 PDF、传统及新版 Office、OpenDocument、RTF、EPUB、iWork、常见思维导图，以及 TXT、Markdown、RST、CSV、TSV；媒体和未知格式保持跳过。`copy-doc` 不改源；`move-doc` 会移动文件并删除已确认重复的源文件。

文档默认模板为 `{category}/{year}/{month}`。除媒体模板占位符外，还支持 `{category}`。模板包含 `{category}` 且配置了类目时，按内容分类；置信度不足时归入 `uncategorized`。不需要分类时，可改用不含 `{category}` 的模板，例如：

```bash
tidymedia copy-doc -o <OUT> --archive-template '{year}/{month}' <SOURCES...>
```

## 查找重复文件

```bash
tidymedia find <SOURCES...> > <REVIEW_SCRIPT>
tidymedia find --secure -o <KEEP_DIR> <SOURCES...> > <REVIEW_SCRIPT>
```

- `find` 只生成 Python 删除脚本，不执行删除；未经逐行审查不得执行。
- 默认使用快速判重；`--secure` 使用更严格的内容判等，速度较慢。
- 不传 `-o` 时，所有删除行默认注释。传入 `-o/--output` 指定保留目录后，保留目录之外的删除行可能默认启用。
- 执行前逐行审查：给不想删除的启用行补回注释，只取消确认可删除的保护行注释，再用 Python 执行。带 `SURVIVOR` 标记的保护行不要放开，否则可能删光某组副本。

## 整理含文字的图片

```bash
tidymedia move-text-shot -o <OUT> --dry-run <SOURCES...>
tidymedia move-text-shot -o <OUT> <SOURCES...>
```

检测图片中的文字，将命中的图片按源目录相对路径移到输出目录；非图片和未命中文件不移动。此命令会改动源文件，正式执行前先预演。

## 筛选相似照片

```bash
tidymedia cull -o <OUT> --dry-run <SOURCES...>
tidymedia cull -o <OUT> [--phash-max <N>] [--report <REPORT>] <SOURCES...>
```

将相似或连拍照片分组，保留评分较高的一张，并把其余候选照片移到 `output/<最佳照片相对源目录>/group-NNN/`；同时放置 `BEST_` 前缀的最佳照片副本供对比。单图、被清晰度门槛剔除的照片，以及过滤后不足两张的组保留在源位置。正式执行会移动源文件。`--phash-max` 越大，分组条件越宽松。

## 归档前核对

```bash
tidymedia verify -o <OUT> [--report <REPORT>] <SOURCES...>
tidymedia verify -o <OUT> --include-non-media <SOURCES...>
```

`verify` 不移动或修改源文件及归档媒体，用于检查预测归档位置、时间冲突、目标库重复情况及修复建议。指定 `--report` 时仍会写报告文件。视频修复必须同时核对 QuickTime `CreationDate` 与 `CreateDate`，不能直接执行只覆盖其中一个字段的建议；操作系统时区与配置时区不同时，基于文件修改时间的交叉对账结果不可直接采用。高级选项 `--exif-tsv` 和 `--phash-max` 可用 `tidymedia verify --help` 查看。

## 配置

默认读取进程当前工作目录下的 `config.yaml`；从其他目录运行时，通过 `TIDYMEDIA_CONFIG=<PATH>` 指定配置文件。所有配置都有内置默认值，未找到配置文件时会静默使用这些默认值。

```yaml
copy:
  timezone_offset_hours: ${TIDYMEDIA_TIMEZONE_OFFSET_HOURS:-8}
  archive_template: "${TIDYMEDIA_ARCHIVE_TEMPLATE:-{year}/{month}/{valuable_name}}"
  doc_archive_template: "${TIDYMEDIA_DOC_ARCHIVE_TEMPLATE:-{category}/{year}/{month}}"
```

- `timezone_offset_hours`：解释无时区的拍摄时间和确定归档年月时使用的整小时时区偏移。
- 归档模板以 `{` 开头时，YAML 值必须加引号。
- `RUST_LOG` 的日志级别优先级高于 `--log-level`，后者高于配置文件。
- 其余跳过扩展名、重名尝试次数、模型路径、分类类目和阈值，直接参考 `config.yaml` 中的说明按需修改。

## 安全操作建议

1. 先备份重要数据，选择一个确实较小且具有代表性的目录做预演；工具不会自动限制文件数量。
2. 检查预演报告和归档年月，再正式执行。典型顺序为 `copy --dry-run`、`verify`、正式 `copy` 或 `move`、最后用 `find` 审查目标库重复。
3. `move`、`move-doc`、`move-text-shot`、`cull` 都会移动或删除源文件，且不进入回收站；其中移动类归档在发现目标已有相同内容时也可能直接删除重复源。
4. `copy`、`copy-doc`、`find` 不修改源文件；`verify` 不修改源或归档媒体。
5. 尽量不要让来源和输出目录重叠。来源位于输出目录内会被拒绝；输出位于来源内时，已归档子树会被排除。
6. 跨卷或远端移动中断时，可能留下未完成副本。保留现场并结合报告核对源和目标，不要仅凭文件大小删除文件；排除故障后再重跑。同卷移动通常减少数据复制和额外空间，但完整过程仍需扫描和判重，不保证秒级完成。
7. 重跑不会产生正常重复副本，但当前没有增量状态，未变化文件也可能重新扫描和判重。

## 当前使用限制

- 本地隐藏文件可能未被扫描；不要据此假定目录已全量处理，应检查报告和源目录。
- 空文件、不可读文件及被配置排除的文件会跳过，相关计数或错误以报告和日志为准。
- 远端大文件读写目前可能把单个文件整体装入内存，部分远端写入路径还有单文件 2 GiB 上限，超限会报错。单个文件过大时，拆分目录也不能保证安全，应先改用其他方式导入或转存。
- 手机内容确认已完整导入并抽样验收后，再自行决定是否清理手机；不要在 ADB 来源上使用移动类命令。
