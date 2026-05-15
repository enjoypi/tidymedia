use std::collections::BTreeMap;
use std::io;
use std::io::Write;

use camino::Utf8PathBuf;
use tracing::error;
use tracing::info;

use super::entities::common;
use super::entities::file_index;
use super::entities::file_info;

pub fn find_duplicates(
    fast: bool,
    sources: Vec<Utf8PathBuf>,
    output: Option<Utf8PathBuf>,
) -> common::Result<()> {
    let mut index = file_index::Index::new();

    if let Some(o) = output.as_ref() {
        let p = std::path::Path::new(o.as_str());
        if !p.is_dir() {
            error!("output is not a directory");
            return Ok(());
        }
    }

    for path in sources {
        index.visit_dir(path.as_str());
    }

    info!(
        "Files: {}, Similar Files: {}, Bytes Read: {}",
        index.files().len(),
        index.similar_files().len(),
        index.bytes_read(),
    );

    let same = if fast {
        index.fast_search_same()
    } else {
        index.search_same()
    };
    info!("Same: {}", same.len());

    let prefix_owned = output
        .as_ref()
        .map(|o| file_info::full_path(o.as_str()).map(|p| p.as_str().to_string()))
        .transpose()?;

    render_script(
        &same,
        prefix_owned.as_deref(),
        comment(),
        rm(),
        &mut std::io::stdout(),
    )?;

    info!("Bytes Read: {}", index.bytes_read());
    Ok(())
}

pub(crate) fn render_script(
    same: &BTreeMap<u64, Vec<Utf8PathBuf>>,
    output_prefix: Option<&str>,
    comment_token: &str,
    rm_token: &str,
    sink: &mut impl Write,
) -> io::Result<()> {
    for (size, paths) in same.iter().rev() {
        writeln!(sink, "{}SIZE {}\r", comment_token, size)?;
        for path in paths.iter() {
            let path_str = path.as_str();
            let starts = output_prefix.is_some_and(|p| path_str.starts_with(p));
            if output_prefix.is_some() && !starts {
                writeln!(sink, "{} \"{}\"\r", rm_token, path)?;
            } else {
                writeln!(sink, "{}{} \"{}\"\r", comment_token, rm_token, path)?;
            }
        }
        writeln!(sink)?;
    }
    Ok(())
}

pub(crate) fn comment() -> &'static str {
    if cfg!(target_os = "windows") {
        ":"
    } else {
        "#"
    }
}

pub(crate) fn rm() -> &'static str {
    if cfg!(target_os = "windows") {
        "DEL"
    } else {
        "rm"
    }
}
