# exiftool 抽取与写回

## 抽取

- 8 列 TSV 契约由 skill `config.yaml` 的 `exiftool_tsv_p` 单点定义，列序对齐 tidymedia 的 P0–P4；修改时与 `src/usecases/verify/exif_tsv.rs`、`src/adapters/cli_commands.rs` 三处同步。值中的 `\t` 由脚本替换为真实 tab。
- 禁止添加 `-fast2`，否则可能跳过 QuickTime moov atom，漏读容器时间。
- 并行抽取使用 argfile 和 `-charset filename=utf8`。Windows 中文路径的 ANSI/GBK 输出由 `scripts/lib/gbk.ts` 规范化，不得删除该兼容层。
- 按目录文件数分组，TSV 先分组写入再合并。并发同时受配置上限和可用内存约束；残留进程通过 pid 文件在下轮清理。

## Step 3 写回

命令、确认项和停止条件以 SKILL 为准，本节只补充细节。

- FileModifyDate 显式附加 `<OFFSET>`，避免 exiftool 按系统时区解释。
- 保留 `_original` 备份时，备份文件若留在媒体目录，后续可能被当作媒体归档。
- 视频必须根据现有证据同时核对并修正 `-QuickTime:CreationDate=` 与 `-QuickTime:CreateDate=`；CreationDate 优先，旧值未处理会掩盖对 CreateDate 的修复。Rust `fix_suggestion` 若只列一个字段，不得直接照抄。
- QuickTime 使用 UTC：写入值由期望本地时间按配置时区换算，不固定减 8 小时。只有日级证据时，按用户确认的时刻换算并检查是否跨日。
- 内容实为 JPEG、扩展名却是 `.png` 的文件可能被 exiftool 拒绝；确认内容后可临时改为 `.jpg` 写回再恢复扩展名，tidymedia 仍按内容识别。
