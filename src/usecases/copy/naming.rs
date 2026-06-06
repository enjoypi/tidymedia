//! 目标命名策略：archive\_template 渲染 + 拍摄时间展开 + 冲突追加序号。

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
) -> Option<(Location, Location)> {
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

    let sub_dir_path = if sub_dir_rel.is_empty() {
        output_dir.path().to_path_buf()
    } else {
        output_dir.path().join(&sub_dir_rel)
    };
    let sub_dir_loc = output_dir.with_path(sub_dir_path.clone());

    let max_attempts = config().copy.unique_name_max_attempts;
    for i in 0..max_attempts {
        let target_path = if i == 0 {
            sub_dir_path.join(file_name)
        } else {
            let mut name = file_stem.clone();
            name.push('_');
            name.push_str(i.to_string().as_str());
            name.push('.');
            name.push_str(ext.as_str());
            sub_dir_path.join(name)
        };
        let target_loc = output_dir.with_path(target_path);

        // 对远端 backend 也通过 backend.exists 检测；同 backend 实例对 Local 等价。
        if !output_backend.exists(&target_loc).unwrap_or(false) {
            return Some((sub_dir_loc, target_loc));
        }
    }
    None
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
