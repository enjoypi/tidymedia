//! 目标命名策略：archive\_template 渲染 + 拍摄时间展开 + 冲突追加序号。

use std::io;
use std::sync::Arc;

use camino::Utf8Component;
use camino::Utf8Path;
use time::OffsetDateTime;

use super::run::{MONTH, configured_offset};
use crate::entities::backend::Backend;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;
use crate::usecases::archive_template::{TemplateContext, render};
use crate::usecases::config::config;

pub(super) fn generate_unique_name(
    src_file: &Info,
    output_dir: &Location,
    output_backend: &Arc<dyn Backend>,
    template: &str,
) -> io::Result<Option<(Location, Location)>> {
    let display_path = Utf8Path::new(src_file.full_path.as_str());
    let file_name = display_path
        .file_name()
        .expect("Info::open guarantees file path has a name");
    let file_stem = display_path
        .file_stem()
        .expect("file with name must have a stem")
        .to_string();
    let ext = display_path.extension().unwrap_or("").to_string();

    let create_time = src_file.create_time(config().exif.valid_date_time_secs);
    let dt = OffsetDateTime::from(create_time).to_offset(configured_offset());
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

    // render 输出以 '/' 分隔；整串 join 会把 '/' 原样嵌进路径，Windows 下产生
    // `D:\Pictures\2003/09` 混合分隔符。逐段 join 让分隔符回到 OS 原生形态。
    let sub_dir_path = sub_dir_rel
        .split('/')
        .filter(|seg| !seg.is_empty())
        .fold(output_dir.path().to_path_buf(), |p, seg| p.join(seg));
    let sub_dir_loc = output_dir.with_path(sub_dir_path.clone());

    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..max_attempts {
        let target_path = if i == 0 {
            sub_dir_path.join(file_name)
        } else {
            // 无扩展名文件不拼 '.'：尾点文件名在 Linux 是怪文件，Windows 下
            // CreateFile 会剥掉尾点，使 exists 判定与实际创建路径不一致。
            let name = if ext.is_empty() {
                format!("{file_stem}_{i}")
            } else {
                format!("{file_stem}_{i}.{ext}")
            };
            sub_dir_path.join(name)
        };
        let target_loc = output_dir.with_path(target_path);

        // 对远端 backend 也通过 backend.exists 检测；同 backend 实例对 Local 等价。
        // exists 的 IO 错误（网络抖动等）必须传播：若吞成"不存在"，后续 open_write
        // 会 truncate 覆盖已存在目标，move 模式下源随后被删即永久数据丢失。
        if !output_backend.exists(&target_loc)? {
            return Ok(Some((sub_dir_loc, target_loc)));
        }
    }
    Ok(None)
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
