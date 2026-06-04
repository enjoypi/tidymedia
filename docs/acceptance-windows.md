# Windows 用户视角验收 tidymedia：600G 照片库 → 全新库

## Context

- 环境：Windows + cmd.exe，本机已有 `tidymedia.exe`
- 数据：本机单盘 ~600 GB 真实照片 / 视频库
- 目标：**copy** 到 1 份全新归档库（原库保留），目标盘 ≥ 720 GB 独立空间
- 关键约束：
  1. 数据量大、单盘 → I/O 是瓶颈，**不要**让 source 与 output 同盘
  2. **绝对不用 `move`**（破坏原库不可逆）
  3. tidymedia copy 是**幂等**的（命中已存在文件即跳过），所以中断可重启；但跑全库前**必须**先小样本验证归档模板与时区设置，避免归到错误目录后整库返工
  4. 中文/Unicode 路径需在 cmd 里 `chcp 65001` 切 UTF-8

## 变量

每个步骤前确认下面三个变量。后文统一引用：

```cmd
set BIN=C:\path\to\tidymedia.exe
set SRC=D:\Photos                    REM 你的 600G 库根
set OUT=E:\PhotosTidied              REM 目标盘新库，独立分区
set LOG=%USERPROFILE%\tidymedia-logs
mkdir %LOG% 2>nul
chcp 65001 >nul
```

> 检查：`%BIN% --version` 能跑、`dir %SRC%` 能列、`%OUT%` 所在盘 free space ≥ 720 GB（`fsutil volume diskfree E:`）。

## 渐进式六阶段

每阶段都是 **read + dry-run** 优先，看完输出再决定是否进下一阶段。任何阶段不符合预期都可以 abort，原库零损伤。

### 阶段 A. 极小样本（5 分钟）— 验工具是否能跑

随机抽 30~50 张到隔离目录，确认 exe 在你环境下基本可用。

```cmd
set ACC=%TEMP%\tidymedia-acc
mkdir %ACC%\tiny %ACC%\tiny-out
xcopy "%SRC%\<随便一个相册>\*.jpg" %ACC%\tiny\ /Y >nul
xcopy "%SRC%\<另一个相册>\*.mp4" %ACC%\tiny\ /Y >nul

%BIN% copy -o %ACC%\tiny-out --dry-run %ACC%\tiny
%BIN% copy -o %ACC%\tiny-out --report %ACC%\tiny.json %ACC%\tiny
type %ACC%\tiny.json
dir /s /b %ACC%\tiny-out
```

**检查点**：

- target 路径形如 `tiny-out\年\月\<valuable_name>\xxx.jpg`
- 年月与你**预期**一致（北京时间默认 +8 时区，跨夜照片不要漂错月）
- 没有 `errors`、`failed=0`
- 非媒体（`.aae` / `.thumbs.db` / `.ini` 等）在 stderr warn 中被跳过

不符合预期 → 调 §阶段 A.1 修参数。

#### A.1 调归档模板 / 时区（如需）

```cmd
REM 时区改 UTC+9
set TIDYMEDIA_TIMEZONE_OFFSET_HOURS=9
%BIN% copy -o %ACC%\tiny-out --dry-run %ACC%\tiny

REM 改模板加 day
set TIDYMEDIA_ARCHIVE_TEMPLATE={year}/{month}/{day}
%BIN% copy -o %ACC%\tiny-out --dry-run %ACC%\tiny

REM 完成调参后写到一个 .cmd 启动脚本里固化，避免后续遗漏
```

确定最终模板/时区后写入 `run-tidy.cmd`：

```cmd
@echo off
chcp 65001 >nul
set TIDYMEDIA_TIMEZONE_OFFSET_HOURS=8
set TIDYMEDIA_ARCHIVE_TEMPLATE={year}/{month}/{valuable_name}
%BIN% %*
```

后续步骤统一 `run-tidy.cmd copy ...`，确保所有跑都同一套配置。

### 阶段 B. 中等子集（5~20 GB，约 15~30 分钟）— 验归档结构 & 性能基线

挑一个有代表性的相册或一整年（含截图、视频、有 EXIF / 无 EXIF / 旁车 xmp 都最好）：

```cmd
set SUB=%SRC%\2023
run-tidy.cmd copy -o %ACC%\sub-out --dry-run --report %ACC%\sub-dry.json "%SUB%" > %LOG%\sub-dry.txt 2> %LOG%\sub-dry.err
type %ACC%\sub-dry.json
```

人工抽看 `%LOG%\sub-dry.txt` 中前 50 行的 `src → target` 配对，确认归档目录树合理。

无问题再实跑：

```cmd
run-tidy.cmd copy -o %ACC%\sub-out --report %ACC%\sub.json "%SUB%" > %LOG%\sub.txt 2> %LOG%\sub.err
type %ACC%\sub.json
```

**检查点**：

- `copied + ignored = scanned`，`failed=0`
- 用 `dir /s /-c %ACC%\sub-out | findstr 个文件` 与 `dir /s /-c %SUB% | findstr 个文件` 对比文件数差异 = `ignored`（这些是同源库内部重复或非媒体）
- 重跑同命令：`copied=0`（幂等验证）

记录此阶段耗时与 GB/min 速率，估算全库时长（线性外推）。

### 阶段 C. 全库 `find` 只读扫描（产出去重报告）

只读、不写 output，但会全库 SHA-512（HDD ~1 小时 / SSD ~20 分钟）。先看一眼你库的重复占比，决定后续策略。

```cmd
run-tidy.cmd find --secure --report %LOG%\find-full.json "%SRC%" > %LOG%\find-script.cmd 2> %LOG%\find.err
type %LOG%\find-full.json | findstr /c:"groups"
```

`%LOG%\find-script.cmd` 是一段可执行的 `DEL` 脚本（**不要**直接跑），先 `more` / 用编辑器看一眼，了解：

- 重复总数 / 重复占用空间（`find-full.json` 的 groups 数组每个有 `size` + N 个 `paths`）
- 重复主要落在哪些子目录（备份目录？旧手机导入？iCloud 同步副本？）

> 用途：如果重复率 < 5%，直接走阶段 D 全库 copy（去重收益小，省事）。
> 如果重复率 > 20%，可以考虑先 review `find-script.cmd` 手动删掉明显重复，缩小阶段 D 的实际写入量。
> **不强制**，只是省时间和目标盘空间。

### 阶段 D. 全库 `copy --dry-run`（可选 sanity check，再扫一次）

```cmd
run-tidy.cmd copy -o %OUT% --dry-run --report %LOG%\full-dry.json "%SRC%" > %LOG%\full-dry.txt 2> %LOG%\full-dry.err
type %LOG%\full-dry.json
```

> 注意：dry-run 会再扫一次 SHA-512（同阶段 C 耗时）。如果阶段 C 已经看清楚情况，**可跳过阶段 D 直接进 E**——E 实跑本身也是哈希扫一遍，多扫一次纯浪费 I/O。

留作 dry-run 价值：能产出**完整**的 src→target 配对清单（`%LOG%\full-dry.txt`），便于事后审计。

### 阶段 E. 全库实跑 copy（核心步骤）

```cmd
fsutil volume diskfree %OUT:~0,2%        REM 跑前再确认 free space
run-tidy.cmd copy -o %OUT% --report %LOG%\full.json "%SRC%" > %LOG%\full.txt 2> %LOG%\full.err
```

预计耗时：

- HDD：~1.5~2 小时（数据吞吐 ~100 MB/s）
- SSD：~25~40 分钟（~300+ MB/s）
- 同盘 source+output：×2 慢，再次提醒目标盘必须独立

**期望**：

- 进程不中断、`%LOG%\full.json` 显示 `failed=0`、`errors:[]`
- `copied + ignored = scanned`

#### 中断恢复

任何中断（断电 / Ctrl+C / 蓝屏 / 误关）后：直接同命令重跑即可。已经在 `%OUT%` 落盘的文件会在第二次扫描时被识别为「已存在重复」跳过，不会再写。代价是要把已扫文件**重新哈希**一遍（这是 tidymedia 的 trade-off：state-less，无 `--state` 增量功能，README roadmap 标了 TODO）。

#### 进度观察（另开一个 cmd）

```cmd
:loop
fsutil volume diskfree %OUT:~0,2%
dir /s /-c %OUT% | findstr 个文件
timeout /t 60
goto loop
```

每分钟看 free space 和 output 文件数变化。

### 阶段 F. 验收新库

```cmd
REM 1. 大小对比（新库应 ≤ 原库，差异 = 去重 + 非媒体跳过）
dir /s /-c %SRC% | findstr 个字节
dir /s /-c %OUT% | findstr 个字节

REM 2. 文件数对比
dir /s /b /a-d %SRC% | find /v /c ""
dir /s /b /a-d %OUT% | find /v /c ""

REM 3. report 总览
type %LOG%\full.json

REM 4. 随机抽样：打开几个新库子目录人眼检查
explorer %OUT%\2023\05
explorer %OUT%\2018\08
explorer %OUT%\2010\01
```

抽样要看：

- 该月相册里照片是不是真的拍摄于该月（看 EXIF / 文件名时间戳）
- 是否有大量「无 EXIF 回退 mtime」误归（mtime 被改过的旧照片会归到错月——这是已知限制，README 与 spec 都说明了）
- 视频与图片混合归档无遗漏

### 阶段 G. 决策

新库 review 通过 → 你可以**手动**决定后续：

- 保留原库做冷备（推荐若磁盘空间允许）
- 或对原库执行 `find-script.cmd`（阶段 C 输出）的删除部分（注释 → 取消注释 → `cmd %LOG%\find-script.cmd`）
- **不**用 tidymedia `move` 命令做全库迁移——其语义对你这个场景过于激进

## 失败/异常应对

| 现象 | 处置 |
|---|---|
| 中途 `failed > 0`，`errors` 非空 | 看 `%LOG%\full.err`：通常是个别文件锁定 / 权限 / 损坏。修复后重跑（幂等） |
| 目标盘空间不足 | 暂停（关 cmd 即可），腾空间或换盘后重跑 |
| 某些文件归到 `unknown\unknown\` 或 1970 年 | 这些文件无 EXIF 也无可信 mtime，spec 行为如此。可单独挑出来人工归档 |
| `valid_date_time_secs` 默认 2000-01-01 把 90 年代老照片误判 | `set TIDYMEDIA_VALID_DATE_TIME_SECS=0` 重跑（接受任何 EXIF 时间） |
| 文件名乱码（中文） | 确保 cmd 是 `chcp 65001`；目录名含 emoji 时 Windows 文件系统层面就限制，与 tidymedia 无关 |
| 进程吃满 I/O，电脑卡顿 | tidymedia 用 rayon 多线程哈希；可 `start /low /b run-tidy.cmd copy ...` 降优先级 |

## 通过判定

阶段 A → B → C → E → F 顺序走完，每阶段检查点全过，并且 §F 人工抽样新库 ≥ 3 个月份目录都是「该月真的拍摄于该月」→ 验收通过。

## 清理

```cmd
rmdir /s /q %TEMP%\tidymedia-acc
REM %LOG% 建议保留 30 天，作为本次迁移的审计证据
```
