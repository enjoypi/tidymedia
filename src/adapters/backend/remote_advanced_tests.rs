//! `remote.rs` 进阶分支：`RemoteBufferedWriter` / `from_location_err` 系列 / root context 跳 mkdir。
//! 从 `remote_tests.rs` 拆出避免单文件 > 512 行。

use std::io;
use std::sync::Arc;

use super::test_helpers::{
    DummyAdapter, DummyClient, DummyCtx, DummyTarget, backend_with_client,
    backend_with_from_loc_err, backend_with_root_ctx, loc,
};
use super::*;

include!("remote_advanced_part1_tests.rs");
include!("remote_advanced_part2_tests.rs");
