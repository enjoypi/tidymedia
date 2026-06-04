# Windows 验收 tidymedia：Android → 本地新库 + 本地 600G 移动整理

## Context

- 环境：Windows + cmd.exe，本机已有 `tidymedia.exe` 与 `adb.exe`（Android Platform Tools）
- 数据：本机单盘 ~600 GB 真实照片 / 视频库 + 一台开了 USB 调试的 Android 手机
- 两条主流程：
  1. **流程 A**：Android → Windows copy（手机 DCIM → 新归档库）
  2. **流程 B**：Windows 本地 move 整理（已有 600G 库按拍摄时间归档到 `年/月`）
- **绝对不要**在 Android 上跑 `move`（`adb://` 源 + `move` 命令）：ADB backend 把整文件读入内存（非流式），且远端删除非事务，中断会双端不一致

## 已知工具限制（先看完再开跑）

| 限制 | 影响 | 规避 |
|---|---|---|
| ADB 非流式：单文件全量 `Vec<u8>` 读入内存 | 4K/8K 视频（>2GB）会让进程内存爆 | 流程 A 先 `find` 看大文件分布，单独拉 Movies/ 不与图片混跑 |
| ADB `timeout_secs` 当前是配置占位，不生效 | USB 抖动 / 设备睡眠会让进程无限挂起 | 进程明显无 I/O 后 Ctrl+C，幂等重跑 |
| Move 不是 rename：所有 backend 上 move = stream copy + 删源 | 中断会留半文件在 OUT，源仍在 | 检查 OUT 半文件大小不等 → 删半文件 → 幂等重跑 |
| CLI 无 `--jobs` flag | rayon 用 CPU 数并发，HDD 上反而抖 | I/O 抢占严重时 `start /low /b ...` 降优先级 |
| `valid_date_time_secs` 默认 2000-01-01 | 90 年代老照片 EXIF 日期被丢弃 | `set TIDYMEDIA_VALID_DATE_TIME_SECS=0` 接受任意 EXIF |

---

## 通用准备

每次开始前固定一次：

```cmd
chcp 65001 >nul
set BIN=C:\path\to\tidymedia.exe
set SRC=D:\Photos
set OUT=E:\PhotosTidied
set LOG=%USERPROFILE%\tidymedia-logs
mkdir %LOG% 2>nul
```

健康检查（任何一项不过 → 不要开跑）：

```cmd
%BIN% --version                                    REM exe 能跑
fsutil volume diskfree %OUT:~0,2%                  REM OUT 盘 free >= 720 GB
adb start-server
adb devices                                        REM 必须看到 device，不是 unauthorized / no permissions
```

固化时区 / 模板到启动脚本 `run-tidy.cmd`（避免每次手敲遗漏）：

```cmd
@echo off
chcp 65001 >nul
set TIDYMEDIA_TIMEZONE_OFFSET_HOURS=8
set TIDYMEDIA_ARCHIVE_TEMPLATE={year}/{month}
%BIN% %*
```

---

## 流程 A：Android → Windows copy

### A0 设备识别

```cmd
adb devices
```

- 输出 0 个设备 → USB 模式切「文件传输 (MTP)」+ 设备解锁 + 信任 PC 弹窗
- 输出 ≥ 2 个设备 → URI **必须**带 serial：`adb://YOUR_SERIAL/sdcard/DCIM`
- 输出 1 个设备 → URI 可以省 serial：`adb:///sdcard/DCIM`

```cmd
set PHONE=adb:///sdcard/DCIM
```

### A1 极小样本（5 分钟）

先只拉 30 张验证工具能跑 + 归档目录对：

```cmd
set TINY_OUT=%TEMP%\tidymedia-tiny
mkdir %TINY_OUT% 2>nul

run-tidy.cmd copy -o %TINY_OUT% --dry-run %PHONE%/Camera > %LOG%\A1-dry.txt
```

检查 `%LOG%\A1-dry.txt` 前 30 行 `src → target` 配对：

- target 形如 `%TINY_OUT%\2024\05\IMG_xxx.jpg`
- 年月与照片**真实拍摄**一致（不是导入手机时间）

无问题再实跑（限定一个相册，~20-50 文件）：

```cmd
run-tidy.cmd copy -o %TINY_OUT% --report %LOG%\A1.json %PHONE%/Camera
type %LOG%\A1.json
```

**通过判定**：`failed=0` + `errors:[]` + `%TINY_OUT%` 下年月树合理 + 几张抽样 EXIF 时间与归档目录吻合。

不过 → 看 §异常应对表。

### A2 中等子集（一个月相册，15-30 分钟）

挑一个月份目录（手机相册按月分文件夹时直接用；若手机不分月则只跑 Camera 但限制时间窗口靠 dry-run 输出筛）：

```cmd
set MED_OUT=%TEMP%\tidymedia-med
mkdir %MED_OUT% 2>nul
run-tidy.cmd copy -o %MED_OUT% --report %LOG%\A2.json %PHONE%/Camera > %LOG%\A2.txt 2> %LOG%\A2.err
```

记录速率：报告里的 `copied` × 平均文件大小 / 实际耗时 = MB/s。用此估算全量 DCIM 耗时（线性外推）。

> ADB 经 USB 2.0：典型 20-30 MB/s；USB 3.0：50-80 MB/s。低于 10 MB/s 说明 USB/线/接口有问题。

### A3 大文件预筛（关键步骤，跳过会爆内存）

```cmd
run-tidy.cmd find --report %LOG%\find-phone.json %PHONE%
```

打开 `find-phone.json`，看有没有单文件 size > 2_000_000_000（2GB）。**有的话**：

1. 单独跑 Movies / 大视频目录，**一次只指定一个子目录**（每次进程结束内存归零）：
   ```cmd
   run-tidy.cmd copy -o %OUT% --report %LOG%\A3-mov.json %PHONE%/Movies
   ```
2. 其余 DCIM / Pictures 再合并跑：
   ```cmd
   run-tidy.cmd copy -o %OUT% --report %LOG%\A3-rest.json %PHONE%/DCIM %PHONE%/Pictures
   ```

无 >2GB 文件 → 一次跑：

```cmd
run-tidy.cmd copy -o %OUT% --report %LOG%\A3-full.json %PHONE%/DCIM %PHONE%/Pictures %PHONE%/Movies
```

进程跑起来后另开 cmd 看 tidymedia.exe 内存占用：

```cmd
tasklist /fi "imagename eq tidymedia.exe" /fo table
```

内存稳定 < 4 GB → 正常。涨到 8 GB+ → Ctrl+C，重跑时进一步拆目录。

### A4 中断恢复

ADB 流程中断（USB 拔了 / 手机锁屏断连 / Ctrl+C）后直接同命令重跑：

- tidymedia copy 幂等：OUT 已有的文件按 SHA-512 命中后跳过（`ignored++`）
- 代价：已扫文件要重新哈希（无 `--state` 增量，README Roadmap TODO）

### A5 抽样验收

```cmd
explorer %OUT%\2024\05
explorer %OUT%\2020\08
explorer %OUT%\2018\12
```

人眼看：

- 该月相册照片**真的拍摄于该月**（看图属性 EXIF）
- 视频与照片混合无遗漏
- 无大量「无 EXIF 回退 mtime」误归（手机相机原图不会，截图 / 微信图常见，可接受）

### A6 手机端清理

新库验收通过后，手机端清理**手动用文件管理器**或 Google Photos 删，**不要**在 `adb://` 上跑 `tidymedia move`：

- ADB 非流式 + 非事务，删源不可回滚
- 手机端删除走 Android MediaStore 索引，文件管理器更可靠

---

## 流程 B：本地 600G 移动整理

> 用前提：你已经有一个目录树乱七八糟的 600G 库（`%SRC%`），想就地按 `年/月` 归档（OUT 与 SRC 可以同盘或异盘）。

### B0 选 OUT

- 同盘（`SRC=D:\Photos`、`OUT=D:\Photos-tidied`）：不需要额外空间（理论上），但**move 不是 rename**，依然走 copy + delete，所以中途会临时占用 OUT 上 = 单文件大小的空间
- 跨盘（`SRC=D:\Photos`、`OUT=E:\Photos-tidied`）：需要 `OUT` 盘 free ≥ 当前 SRC 大小（因为先 copy 再 delete）

**重要**：所有 backend 下 `move` 都是 `copy + delete`，不是文件系统 rename。同盘 move 也要 OUT 盘有足够 free（即便最终空间持平）。

### B1 极小样本

```cmd
set SUB=%SRC%\2023\一些乱七八糟的目录
run-tidy.cmd move -o %OUT% --dry-run --report %LOG%\B1-dry.json "%SUB%" > %LOG%\B1-dry.txt
type %LOG%\B1-dry.json
```

`dry-run` 不动文件。检查 `%LOG%\B1-dry.txt` 配对合理后实跑：

```cmd
run-tidy.cmd move -o %OUT% --report %LOG%\B1.json "%SUB%"
type %LOG%\B1.json
```

**通过判定**：`failed=0` + 源目录该子集文件消失 + OUT 目录归档树合理。

### B2 全库 dry-run（可选 sanity check）

```cmd
run-tidy.cmd move -o %OUT% --dry-run --report %LOG%\B2-dry.json "%SRC%" > %LOG%\B2-dry.txt
```

> 注意：dry-run 也要扫一遍 SHA-512，耗时与实跑相当。如果 B1 已通过，可直接进 B3。

### B3 全库实跑

```cmd
fsutil volume diskfree %OUT:~0,2%
run-tidy.cmd move -o %OUT% --report %LOG%\B3.json "%SRC%" > %LOG%\B3.txt 2> %LOG%\B3.err
```

预计耗时（HDD ~100 MB/s）：600 GB ≈ 100 分钟（同盘）/ 200 分钟（跨盘，I/O 抢占少）。

#### 中断处理（必看）

`move` 过程被打断（Ctrl+C / 断电 / OUT 盘满）会留下这些状态：

1. SRC 上：已处理的文件**可能已经删了**（move 是 copy 成功 → delete 源；如果中断发生在 delete 之前则源还在）
2. OUT 上：可能有**半文件**（copy 写到一半进程被杀）

恢复流程：

```cmd
REM 1. 找 OUT 上 zero-byte 或异常小的文件
forfiles /p %OUT% /s /m *.* /c "cmd /c if @fsize lss 1024 echo @path"

REM 2. 手工核对：上一行列出的文件如果是 jpg/mp4 但 <1KB，几乎必然是半文件，删除
del "<那些半文件路径>"

REM 3. 重跑同命令，tidymedia 幂等：
REM    - 源已删 + OUT 已有同 hash 文件 → 跳过
REM    - 源还在 + OUT 无对应文件 → 重新 copy+delete
REM    - 源还在 + OUT 有半文件（已被你手删）→ 重新 copy+delete
run-tidy.cmd move -o %OUT% --report %LOG%\B3-retry.json "%SRC%"
```

> 关键事实：tidymedia 用 SHA-512 判重，OUT 上的半文件**不**会被识别为"已存在"（hash 不一致），但**也不会**被自动清理。所以中断后必须手工清半文件再重跑。

### B4 幂等验证

紧接 B3 成功后再跑一次同命令：

```cmd
run-tidy.cmd move -o %OUT% --report %LOG%\B4.json "%SRC%"
type %LOG%\B4.json
```

**通过判定**：`copied=0`、`ignored` 等于上一轮 `copied + ignored`、`failed=0`、SRC 已空（或只剩非媒体文件）。

### B5 抽样验收

同 §A5：随机打开 3 个月份目录人眼检查归档正确性。

---

## 异常应对表

| 现象 | 处置 |
|---|---|
| `adb devices` 空 | adb kill-server && adb start-server；手机 USB 改文件传输；解锁屏 + 信任 PC |
| `adb devices` 列出 `unauthorized` | 手机弹窗点「允许」；不弹则 adb kill-server 重来 |
| 多设备未给 serial → tidymedia 报 ambiguous | URI 改 `adb://SERIAL/...`（serial 从 `adb devices` 第一列复制） |
| 进程卡死无任何 I/O 数据移动 | 已知 ADB timeout 占位不生效，Ctrl+C 后幂等重跑 |
| `tidymedia.exe` 内存涨到 8 GB+ | 大视频被读入内存，Ctrl+C，按 §A3 拆目录单跑 |
| OUT 出现 0 字节 / 几 KB 异常小文件 | move 中断半文件，手删后幂等重跑 |
| `failed > 0` 且 `errors` 含路径 | 看 `errors[].message`：文件锁定 / 权限拒绝 / 路径含奇异字符 → 修复后幂等重跑 |
| 中文路径乱码 | 确认 `chcp 65001` 已执行（每个新 cmd 窗口都要重跑） |
| 90 年代老照片归到 1970/01 | EXIF 日期被 `valid_date_time_secs` 默认 2000 截断，`set TIDYMEDIA_VALID_DATE_TIME_SECS=0` 重跑 |
| 跨夜照片归到错的日期 | 时区漂移，`set TIDYMEDIA_TIMEZONE_OFFSET_HOURS=8` 后重跑（默认应该已是 8） |
| 路径超过 260 字符（NTFS MAX_PATH） | OUT 根路径选短一点（如 `E:\T`）；归档模板去掉冗余 `{day}` |

---

## 通过判定（整体）

流程 A：A1 → A3 → A5 三阶段全过 → A 完成
流程 B：B1 → B3 → B4 三阶段全过 → B 完成
人工抽样：A5 + B5 各随机抽 3 个月份目录全对 → 整体验收通过

任何阶段未过 → 按 §异常应对表 修复后**幂等重跑**该阶段，不要跳过。

---

## 清理

```cmd
rmdir /s /q %TEMP%\tidymedia-tiny %TEMP%\tidymedia-med 2>nul
REM %LOG% 建议保留 30 天作为本次迁移审计证据
```
