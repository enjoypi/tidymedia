# tidymedia

Tidy media files. 跨目录扫描、去重、按时间归档。Rust 单二进制 CLI。

## 系统依赖

- 外部 `exiftool`（用于读取 EXIF / MIME 类型）

  ```
  sudo apt-get install -y libimage-exiftool-perl
  ```

## 子命令

### `find`：列出重复文件

扫描 sources 下的文件，把重复组打印为一段删除脚本到 stdout（Linux/Mac 用 `rm`，Windows 用 `DEL`）。
默认用快速非加密哈希 xxh3-64；加 `--secure` 改用 SHA-512。

```
tidymedia find <SOURCES...>             # 默认快速模式
tidymedia find --secure <SOURCES...>    # 严格判等
tidymedia find -o <KEEP_DIR> <SOURCES...>  # 在 KEEP_DIR 下的文件，对应删除命令被注释
```

`-o/--output` 指向的目录视为"保留区"，该目录内文件的删除行会被注释掉，便于人工 review 再执行。

### `copy`：去重复制媒体文件

把 sources 下尚未出现在 output 的媒体文件（image / video，由 exiftool MIME 判定）复制到 output，按 `年/月/有中文的最内层目录名` 分桶。

```
tidymedia copy -o <OUT> <SOURCES...>
tidymedia copy -o <OUT> --dry-run <SOURCES...>
tidymedia copy -o <OUT> --include-non-media <SOURCES...>
```

默认会**静默跳过非媒体**（文档、未识别 raw 等），并在 stderr 记 warn。要一并复制，加 `--include-non-media`。

判重用 SHA-512（fast_hash 初筛 + size + secure_hash），杜绝快速哈希碰撞误判。

### `move`：去重移动

与 `copy` 同语义，但是**物理 move**（成功后源被删）；命中 output 已有重复的源文件会被**直接 rm**（无回收站）。建议先 `--dry-run` 跑一遍。

```
tidymedia move -o <OUT> <SOURCES...>
tidymedia move -o <OUT> --dry-run <SOURCES...>
```

## 行为说明（容易踩的坑）

- **目录遍历不再尊重 `.gitignore` / `.ignore`**：早期版本会继承 ripgrep 风格的 ignore 规则；现在统一关闭，避免媒体目录恰好在 git 工作树里时被静默漏扫。
- **空文件 / 不可读文件被跳过且记 warn**：扫描阶段计数会出现在 `summary` 日志的 `skipped_empty` / `skipped_unreadable` / `walker_errors` 字段。
- **非媒体被跳过**：见 `copy` / `move` 节。
- **`move` 会物理删除源**：判等已经用 SHA-512，理论碰撞概率 1/2^256，但删除不可逆，敏感场景请保留备份。
- **`find` 输出是脚本，不会自动执行**：默认全部删除行已加注释或未注释（取决于 `--output`），用户拿到后自行 `bash | sh`。

## 输出

- 成功复制 / 移动的条目用 `"<source>"<TAB>"<target>"` 写入 stdout，便于管道串行。
- 日志、warn、error、summary 走 stderr。

## 配置

可选 `config.yaml`（项目根；用 `TIDYMEDIA_CONFIG=/path/to.yaml` 指定其他位置）。所有键都有内置默认值，文件缺失不致命。

```yaml
copy:
  timezone_offset_hours: ${TIDYMEDIA_TIMEZONE_OFFSET_HOURS:-8}
  unique_name_max_attempts: ${TIDYMEDIA_UNIQUE_NAME_MAX_ATTEMPTS:-10}
exif:
  valid_date_time_secs: ${TIDYMEDIA_VALID_DATE_TIME_SECS:-946684800}
```

- `timezone_offset_hours`：按年/月分桶时使用的时区（整数小时，越界回退 UTC）
- `unique_name_max_attempts`：目标重名时 `_1` `_2` … 最多尝试次数；用尽后该文件 copy 失败
- `valid_date_time_secs`：EXIF 时间戳低于该 UNIX 秒数视为不可信，回退到文件 mtime

## 开发

- 测试：`cargo nextest run`
- 覆盖率（stable）：`cargo llvm-cov nextest --summary-only`
- 覆盖率（nightly 严格 100%）：`RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov nextest --summary-only`

详见 `CLAUDE.md`。
