//! `Index` 进阶/远端测试：`similar_files` / `exists_secure` / `visit_dir` 高级 / `visit_stats` / `visit_location` 多 backend。
//! 从 `file_index_tests.rs` 拆出避免单文件 > 512 行。

use std::collections::HashSet;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dashmap::DashMap;
use parking_lot::Mutex;
use tempfile::tempdir;

use super::super::file_info;
use super::super::test_common as common;
use super::Index;
use super::Info;

include!("file_index_advanced_part1_tests.rs");
include!("file_index_advanced_part2_tests.rs");
