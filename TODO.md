# TODO

## 覆盖率补至三项 100%（region / line / branch）

2026-06 `--branch` 严格口径基线：region 99.79% / line 99.87% / branch 99.05%，**未达标**。

| 文件 | region | fn | line | branch |
|---|---|---|---|---|
| `src/adapters/backend/local.rs` | 3 | 1 | 1 | — |
| `src/adapters/backend/remote.rs` | 1 | — | 1 | — |
| `src/adapters/dispatch.rs` | 1 | — | — | — |
| `src/entities/file_index.rs` | 1 | — | 1 | 2 |
| `src/entities/file_info/info.rs` | 2 | — | — | — |
| `src/usecases/copy/run.rs` | 2 | — | 1 | 1 |

- branch 缺口先甄别 multi-binary instance 假阳性（套路见 CLAUDE.md「测试与覆盖率」节陷阱条）再补测试
- 复现：`source ./dev-env.sh && RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov --release nextest --summary-only --branch`
- 定位子行 region：`cargo +nightly llvm-cov report --release --text`，找 `^0` 标记
