use std::collections::BTreeMap;
use std::io::Write;

use camino::Utf8PathBuf;
use tracing::error;
use tracing::info;

use crate::entities::common;
use crate::entities::file_index;
use crate::entities::file_info;

const FEATURE_FIND: &str = "find";

pub fn find_duplicates(
    fast: bool,
    sources: Vec<Utf8PathBuf>,
    output: Option<Utf8PathBuf>,
) -> common::Result<()> {
    let mut index = file_index::Index::new();

    if let Some(o) = output.as_ref() {
        let p = std::path::Path::new(o.as_str());
        if !p.is_dir() {
            error!(
                feature = FEATURE_FIND,
                operation = "validate_output",
                result = "not_a_directory",
                output = %o,
                "output path is not a directory"
            );
            return Ok(());
        }
    }

    for path in sources {
        index.visit_dir(path.as_str());
    }

    info!(
        feature = FEATURE_FIND,
        operation = "scan_complete",
        result = "ok",
        fast,
        files = index.files().len(),
        similar_files = index.similar_files().len(),
        bytes_read = index.bytes_read(),
        "index built"
    );

    let same = if fast {
        index.fast_search_same()
    } else {
        index.search_same()
    };
    info!(
        feature = FEATURE_FIND,
        operation = "search_same",
        result = "ok",
        fast,
        groups = same.len(),
        "duplicate groups discovered"
    );

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
    );

    info!(
        feature = FEATURE_FIND,
        operation = "finalize",
        result = "ok",
        bytes_read = index.bytes_read(),
        "find_duplicates done"
    );
    Ok(())
}

pub(crate) fn render_script(
    same: &BTreeMap<u64, Vec<Utf8PathBuf>>,
    output_prefix: Option<&str>,
    comment_token: &str,
    rm_token: &str,
    sink: &mut impl Write,
) {
    for (size, paths) in same.iter().rev() {
        let _ = writeln!(sink, "{}SIZE {}\r", comment_token, size);
        for path in paths.iter() {
            let path_str = path.as_str();
            let starts = output_prefix.is_some_and(|p| path_str.starts_with(p));
            if output_prefix.is_some() && !starts {
                let _ = writeln!(sink, "{} \"{}\"\r", rm_token, path);
            } else {
                let _ = writeln!(sink, "{}{} \"{}\"\r", comment_token, rm_token, path);
            }
        }
        let _ = writeln!(sink);
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn comment() -> &'static str {
    ":"
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn comment() -> &'static str {
    "#"
}

#[cfg(target_os = "windows")]
pub(crate) fn rm() -> &'static str {
    "DEL"
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn rm() -> &'static str {
    "rm"
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use crate::entities::test_common as tc;
    use super::comment;
    use super::find_duplicates;
    use super::render_script;
    use super::rm;

    fn run_render(
        same: &BTreeMap<u64, Vec<Utf8PathBuf>>,
        prefix: Option<&str>,
        c: &str,
        r: &str,
    ) -> String {
        let mut sink: Vec<u8> = Vec::new();
        render_script(same, prefix, c, r, &mut sink);
        String::from_utf8(sink).unwrap()
    }

    fn sample_two_groups() -> BTreeMap<u64, Vec<Utf8PathBuf>> {
        let mut m = BTreeMap::new();
        m.insert(
            100,
            vec![
                Utf8PathBuf::from("/data/small_a"),
                Utf8PathBuf::from("/data/small_b"),
            ],
        );
        m.insert(
            200,
            vec![
                Utf8PathBuf::from("/data/big_a"),
                Utf8PathBuf::from("/data/big_b"),
            ],
        );
        m
    }

    #[test]
    fn render_script_unix_tokens_no_output() {
        let same = sample_two_groups();
        let out = run_render(&same, None, "#", "rm");
        let expected = "#SIZE 200\r\n#rm \"/data/big_a\"\r\n#rm \"/data/big_b\"\r\n\n\
                        #SIZE 100\r\n#rm \"/data/small_a\"\r\n#rm \"/data/small_b\"\r\n\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn render_script_windows_tokens_no_output() {
        let same = sample_two_groups();
        let out = run_render(&same, None, ":", "DEL");
        assert!(out.contains(":SIZE 200\r\n"));
        assert!(out.contains(":DEL \"/data/big_a\"\r\n"));
    }

    #[test]
    fn render_script_uncommments_paths_outside_output_prefix() {
        let same = sample_two_groups();
        let out = run_render(&same, Some("/keepers"), "#", "rm");
        for line in [
            "rm \"/data/big_a\"\r",
            "rm \"/data/big_b\"\r",
            "rm \"/data/small_a\"\r",
            "rm \"/data/small_b\"\r",
        ] {
            assert!(out.contains(line), "missing line: {line}\nfull:\n{out}");
        }
        assert!(!out.contains("#rm \"/data/"));
    }

    #[test]
    fn render_script_keeps_paths_under_output_prefix_commented() {
        let mut m: BTreeMap<u64, Vec<Utf8PathBuf>> = BTreeMap::new();
        m.insert(
            42,
            vec![
                Utf8PathBuf::from("/keepers/a"),
                Utf8PathBuf::from("/other/b"),
            ],
        );
        let out = run_render(&m, Some("/keepers"), "#", "rm");
        assert!(out.contains("#rm \"/keepers/a\"\r"));
        assert!(out.contains("rm \"/other/b\"\r"));
        assert!(!out.contains("#rm \"/other/b\""));
    }

    #[test]
    fn render_script_descending_size_order() {
        let same = sample_two_groups();
        let out = run_render(&same, None, "#", "rm");
        let idx_200 = out.find("SIZE 200").unwrap();
        let idx_100 = out.find("SIZE 100").unwrap();
        assert!(idx_200 < idx_100);
    }

    #[test]
    fn render_script_empty_input_writes_nothing() {
        let empty: BTreeMap<u64, Vec<Utf8PathBuf>> = BTreeMap::new();
        let out = run_render(&empty, None, "#", "rm");
        assert!(out.is_empty());
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn comment_and_rm_unix_tokens() {
        assert_eq!(comment(), "#");
        assert_eq!(rm(), "rm");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn comment_and_rm_windows_tokens() {
        assert_eq!(comment(), ":");
        assert_eq!(rm(), "DEL");
    }

    #[test]
    fn find_duplicates_invalid_output_returns_ok() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        find_duplicates(
            true,
            vec![Utf8PathBuf::from(tc::DATA_DIR)],
            Some(Utf8PathBuf::from(tmp.path().to_str().unwrap())),
        )
        .unwrap();
    }

    #[test]
    fn find_duplicates_no_output_branch_runs() {
        find_duplicates(true, vec![Utf8PathBuf::from(tc::DATA_DIR)], None).unwrap();
    }

    #[test]
    fn find_duplicates_with_output_branch_runs() {
        let dir = tempdir().unwrap();
        let out = Utf8PathBuf::from(dir.path().to_str().unwrap());
        find_duplicates(false, vec![Utf8PathBuf::from(tc::DATA_DIR)], Some(out)).unwrap();
    }
}
