# exiftool 抽取与写回

## 抽取

- 8 列 TSV 契约由 skill `config.yaml` 的 `exiftool_tsv_p` 单点定义，并与 `src/usecases/verify/exif_tsv.rs`、`src/adapters/cli_commands.rs` 保持同步。
- 禁止添加 `-fast2`，否则可能跳过 QuickTime moov atom，漏读容器时间。
- 并行抽取使用 argfile 和 `-charset filename=utf8`。Windows 中文路径的 ANSI/GBK 输出由 `lib/gbk.ts` 规范化，不得删除该兼容层。
- 并发同时受配置上限和可用内存约束；残留进程通过 pid 文件在下轮清理。
- Windows 使用仓内 `bin/exiftool/exiftool.exe`；macOS 使用用户安装的 `exiftool`。
- `run.ts` 缺少 exiftool 时跳过 TSV 并继续 verify；`collect_evidence.ts` 缺少 exiftool 时退出 2，因为无法收集所需证据。两者不得混写。

## Step 3 写回

默认建议同写 AllDates 与 FileModifyDate，但是否执行、是否保留备份及字段范围均由用户确认。

Windows 示例：

```bash
bin/exiftool/exiftool.exe -P -overwrite_original "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=YYYY:MM:DD HH:MM:SS<OFFSET>" "<file>"
```

macOS 将可执行文件路径替换为 `exiftool`。

- `<OFFSET>` 为 FileModifyDate 显式附加配置时区偏移，例如 UTC+8 写 `+08:00`，避免 exiftool 按其他系统时区解释。若操作系统时区与配置不同，FsMtime 对账本身也可能失真，必须先按 SKILL 的时区前置检查处理。
- `-overwrite_original` 不留 `_original` 备份，只有用户选择该方式时才能使用；备份文件若留在媒体目录，后续可能被当作媒体归档。
- 视频必须根据现有证据同时核对并修正 `-QuickTime:CreationDate=` 与 `-QuickTime:CreateDate=`；CreationDate 优先，旧值未处理会掩盖对 CreateDate 的修复。Rust `fix_suggestion` 若只列一个字段，不得直接照抄。
- QuickTime 使用 UTC：写入值由期望本地时间按当前配置时区换算，不固定减 8 小时。只有日级证据时，按用户确认的默认时刻换算并检查是否跨日。
- 内容实为 JPEG、扩展名却是 `.png` 的文件可能被 exiftool 拒绝；确认内容后可临时改为 `.jpg` 写回再恢复扩展名，tidymedia 仍按内容识别。
- 写回后必须重新执行 Step 1，不能使用旧 TSV 推断修复成功。
