# exiftool 抽取与写回

## 抽取（extract_exif.ts）

- 8 列契约在 `config.yaml` 的 `exiftool_tsv_p`（契约单点，与
  `src/usecases/verify/exif_tsv.rs`、`src/adapters/cli.rs` 互指同步）。
- MUST NOT 加 `-fast2`：会跳过 QuickTime moov atom 致 QT 时间读不到，老 QuickTime
  （pnot 起头 MOV）被误判桶一致。
- Windows Perl 对含中文入口路径按 ANSI(GBK) 输出文件名字节；脚本已统一规范化为
  UTF-8（`lib/gbk.ts`），勿移除。
- 平台探测：Windows 用 repo 内 `bin/exiftool/exiftool.exe`；macOS 需自装
  （homebrew `brew install exiftool`），缺失时脚本退出 2 且交叉比对跳过。

## 写回（Step 5，手工执行）

默认「不留备份 + 同写 AllDates 与 FileModifyDate」：

```bash
bin/exiftool/exiftool.exe -P -overwrite_original \
  "-AllDates=YYYY:MM:DD HH:MM:SS" "-FileModifyDate=YYYY:MM:DD HH:MM:SS" "<file>"
```

- 不留 `_original` 备份：`-overwrite_original`，否则残留备份会被 tidymedia 当
  JPEG 归档。
- 需要保留 `_original` 或调整字段范围时直接单跑 exiftool。
- **视频 MUST 显式 `-QuickTime:CreateDate=`**：`-AllDates` 对 mp4 落 XMP 而非
  QuickTime mvhd，tidymedia 读不到。QT 时间是 UTC 语义：写入值 = 期望本地时间
  − 8h（日精度建议本地 12:00 → UTC 04:00 防跨界）。
- **微信伪 png**（内容 JPEG、扩展名 `.png`）：exiftool 报 `Not a valid PNG` 拒写
  （`-m` 无效）→ 临时改名 `.jpg` 写入后改回；tidymedia 按 magic bytes 嗅探不受影响。
- **Windows 中文路径**：exiftool 按 ANSI(GBK) 输出文件名字节，读回用 `lib/gbk.ts`
  规范化口径。
