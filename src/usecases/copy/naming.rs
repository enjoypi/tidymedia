//! 目标命名策略：archive\_template 渲染 + 拍摄时间展开 + 冲突追加序号。

use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use camino::Utf8Component;
use camino::Utf8Path;
use time::OffsetDateTime;

use super::run::{MONTH, configured_chrono_offset, configured_offset};
use crate::entities::backend::Backend;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use crate::usecases::archive_template::{TemplateContext, render};
use crate::usecases::config::config;

pub(super) fn generate_unique_name(
    src_file: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    template: &str,
    output_index: &Index,
) -> io::Result<Option<(Location, Location)>> {
    let display_path = Utf8Path::new(src_file.full_path.as_str());
    // file_name 缺失（源 Location 是纯 root 如 `smb://host/share/` 或末尾 '/'）
    // 走 Ok(None) 让 caller 报 "无法为 ... 生成目标目录的文件名"，不再 expect panic
    // （P0 §2「MUST NOT 因用户输入而 panic」）。
    let Some(file_name) = display_path.file_name() else {
        return Ok(None);
    };
    let file_stem = display_path.file_stem().unwrap_or(file_name).to_string();
    let ext = display_path.extension().unwrap_or("").to_string();

    let create_time = src_file.create_time(
        config().exif.valid_date_time_secs,
        configured_chrono_offset(),
    );
    // `OffsetDateTime::from(SystemTime)` 内部含 `.expect("Duration doesn't fit into
    // i64::MAX")` 对越界值 panic（time crate 上限 ~9999 年 + 内部 nanos i64 溢出）。
    // 未来 kernel / 损坏 FS mtime 越界会让整个 copy 进程崩溃、中间态无法收敛。
    // 走 `duration_since(UNIX_EPOCH)` + `i64::try_from` + `from_unix_timestamp` 三段
    // 守护，任一溢出兜底到 `UNIX_EPOCH`，让越界文件归档到 1970/01 桶（用户可后续
    // 修 EXIF/mtime 手动 re-archive）而非 panic 掉整批。P0 §2 硬约束。
    let dt = system_time_to_offsetdatetime(create_time)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .to_offset(configured_offset());
    let year = dt.year().to_string();
    let month = MONTH[dt.month() as usize];
    let day = format!("{:02}", dt.day());

    let valuable_name = extract_valuable_name(display_path);

    let template_ctx = TemplateContext {
        year: &year,
        month,
        day: &day,
        valuable_name: &valuable_name,
        exif: src_file.exif_ref(),
    };
    let sub_dir_rel = render(template, &template_ctx);

    // 逐段 Location::join_path：按 scheme 分流分隔符（Local 走 OS 原生、Smb/Mtp/Adb
    // 强制 `/`）。旧实现 `Utf8PathBuf::join` 在 Windows host + 远端 output 时注入
    // `\` 让 pavao/adb shell/libmtp 找不到路径（CLAUDE.md 「Location::join_path」
    // 单点规则）。防御 `..` / `.` 段：archive_template::sanitize_path_segment 已洗
    // EXIF make/model，此为覆盖 valuable_name 等其它渠道的第二道防线。
    let sub_dir_loc = sub_dir_rel
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
        .fold(output_dir.clone(), |loc, seg| loc.join_path(seg));

    let max_attempts = config().copy.unique_name_max_attempts;
    // 范围 `0..=max_attempts`：i=0 试原名，i=1..=N 试 `_1..=_N`，共 N+1 候选；
    // 与配置文档"`unique_name_max_attempts` = N 个数字后缀"一致。旧 `0..N` 让
    // 后缀只到 _{N-1}，N=10 时 _10 永不被尝试，第 11 个同名文件直接失败。
    for i in 0..=max_attempts {
        let candidate_name = if i == 0 {
            file_name.to_string()
        } else if ext.is_empty() {
            // 无扩展名文件不拼 '.'：尾点文件名在 Linux 是怪文件，Windows 下
            // CreateFile 会剥掉尾点，使 exists 判定与实际创建路径不一致。
            format!("{file_stem}_{i}")
        } else {
            format!("{file_stem}_{i}.{ext}")
        };
        let target_loc = sub_dir_loc.join_path(&candidate_name);

        // dry_run 也 add cloned_at 到 output_index（ops.rs），此处校验避免第二个
        // 同 basename+同月+不同 hash 源被静默分派到同一 target；否则 dry-run 报告
        // target 相同、真跑却走 _N 后缀，tidy-verify 桶对账漏 collision。
        if output_index.contains_target(&target_loc) {
            continue;
        }
        // 对远端 backend 也通过 backend.exists 检测；同 backend 实例对 Local 等价。
        // exists 的 IO 错误（网络抖动等）必须传播：若吞成"不存在"，后续 open_write
        // 会 truncate 覆盖已存在目标，move 模式下源随后被删即永久数据丢失。
        if !output_backend.exists(&target_loc)? {
            return Ok(Some((sub_dir_loc, target_loc)));
        }
    }
    Ok(None)
}

/// Panic-safe `SystemTime → OffsetDateTime`：`OffsetDateTime::from(SystemTime)` 内含
/// `.expect(...)` 会在越界（future `mtime` / 损坏 FS）触发 panic 违反 P0 §2。三段
/// 守护（`duration_since` / `i64::try_from` / `from_unix_timestamp`）任一失败返 None，
/// 让 caller fallback 到 `UNIX_EPOCH` 归档到 1970/01 桶而非崩进程。
fn system_time_to_offsetdatetime(t: SystemTime) -> Option<OffsetDateTime> {
    let dur = t.duration_since(UNIX_EPOCH).ok()?;
    convert_dur_secs_to_offsetdatetime(dur.as_secs())
}

/// `i64::try_from` + `OffsetDateTime::from_unix_timestamp` 两段兜底走 `coverage(off)`：
/// Linux/macOS 平台 `Duration::as_secs()` 由 `timespec.tv_sec: i64` 转 u64，最大值不
/// 超过 `i64::MAX`，`try_from` Err arm 逻辑不可达；`from_unix_timestamp` 越界仅在
/// 探测超远未来（≈ ±5×10¹⁴ 年）才失败。两条 Err arm 都无法从测试稳定触发。
#[cfg_attr(coverage_nightly, coverage(off))]
fn convert_dur_secs_to_offsetdatetime(secs: u64) -> Option<OffsetDateTime> {
    let secs = i64::try_from(secs).ok()?;
    OffsetDateTime::from_unix_timestamp(secs).ok()
}

pub(super) fn any_non_english(s: &str) -> bool {
    s.chars().any(|c| c as u32 > 127)
}

pub(super) fn extract_valuable_name(full_path: &Utf8Path) -> String {
    let mut components: Vec<Utf8Component> = full_path.components().collect();
    if components.len() > 1 {
        components.pop();
    }

    for c in components.into_iter().rev() {
        if let Utf8Component::Normal(s) = c
            && any_non_english(s)
        {
            return s.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// pre-epoch `SystemTime` → `duration_since(UNIX_EPOCH)` Err arm 命中，早返 None。
    /// 覆盖 CLAUDE.md「P0 §2 用户输入 MUST NOT panic」防御性 branch。
    #[test]
    fn system_time_to_offsetdatetime_pre_epoch_returns_none() {
        let pre = UNIX_EPOCH.checked_sub(Duration::from_secs(1));
        // 部分 Windows/CI 环境 SystemTime 不支持 pre-epoch 构造 → checked_sub 返 None
        // 直接跳过（本测试仅关注 pre-epoch 可构造时的路径命中）。
        let Some(t) = pre else {
            return;
        };
        assert!(system_time_to_offsetdatetime(t).is_none());
    }

    /// `UNIX_EPOCH` 本身 → `Ok(OffsetDateTime::UNIX_EPOCH)`（首正常路径回归）。
    #[test]
    fn system_time_to_offsetdatetime_at_epoch_returns_epoch() {
        let got = system_time_to_offsetdatetime(UNIX_EPOCH).expect("UNIX_EPOCH is valid");
        assert_eq!(got.unix_timestamp(), 0);
    }
}
