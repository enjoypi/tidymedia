# tidymedia justfile - build / nextest / llvm-cov 统一入口
#
# 跨平台：Windows 的 build/test/lint 经 tools/msvc-env.cmd 导入 MSVC 环境
# （vcvars64 + UCRT 补全 + RUSTFLAGS /LIBPATH，固化本机两个坑的解法）；
# Linux/macOS 直接跑 cargo。llvm-cov 完整门禁仅 Linux（--all-features 需 libsmbclient）。
#
# 用法：
#   just build            日常构建（Windows 默认 opt=3，unix 默认 opt=0）
#   just OPT=0 build      快速编译验证（opt=0，编译如 debug）
#   just test             nextest 全量
#   just test xavc        nextest 按名字子串过滤
#   just cov              覆盖率门禁（仅 Linux）
#   just lint / fmt / clean

set shell := ["bash", "-cu"]

# Windows 默认 opt=3（本机真跑为主）；unix 日常 opt=0，真跑 `just OPT=3 <recipe>` 覆盖
OPT := if os() == "windows" { "3" } else { "0" }
export CARGO_PROFILE_RELEASE_OPT_LEVEL := OPT

# Windows 下经 cmd 调 msvc-env.cmd 设环境后执行；unix 直接跑
CMD := 'C:\Windows\System32\cmd.exe'

# 覆盖率 ignore-regex：与 CLAUDE.md「严格 100% 命令」单点同步
COV_IGNORE := '(adapters/backend/[a-z]+_real\.rs|adapters/(ocr|face|classify)/tract_[a-z_]+\.rs)$'

default:
    @just --list

# === 构建 ===
[windows]
build:
    "{{CMD}}" //c "call tools\msvc-env.cmd && cargo build --release"

[unix]
build:
    cargo build --release

# === 测试（nextest）===
# just test            全量（lib + 集成）
# just test <name>     按名字子串过滤，如 just test xavc
[windows]
test *ARGS:
    "{{CMD}}" //c "call tools\msvc-env.cmd && cargo nextest run --release {{ARGS}}"

[unix]
test *ARGS:
    cargo nextest run --release {{ARGS}}

# === 覆盖率（llvm-cov）===
# 完整门禁 = report 四指标 fail-under + lcov DA/BRDA `,0` 真值 plate + 生产文件完整性自检。
# 机制与阈值依据见 CLAUDE.md「测试与覆盖率」。MUST 独占跑（并行 cargo 易 SIGKILL）。
# 跑前先 just fmt + just lint（行号对齐）。
[linux]
cov:
    #!/usr/bin/env bash
    set -euo pipefail
    LCOV_OUT=target/coverage.lcov

    # --profraw-only：只清覆盖率原始数据，保留 instrumented 编译产物（重跑免全量重编）
    cargo llvm-cov clean --profraw-only

    # nextest 必须：进程级单例 + env 覆盖测试在 cargo test 单进程多线程下争用必 fail
    RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov nextest \
      --release --all-features --locked \
      --branch --cargo-quiet --failure-output immediate-final --no-fail-fast \
      --no-report --show-progress none --status-level fail --success-output never \
      --ignore-filename-regex={{COV_IGNORE}}

    # 四指标：file-lines 99.999 兜聚合 float 缺陷；其余全 100。输出只留 <100% 行 + TOTAL
    cargo +nightly llvm-cov report --release \
      --fail-under-file-lines 99.999 \
      --fail-under-functions 100 \
      --fail-under-lines 100 \
      --fail-under-regions 100 \
      | awk '
          /^Filename|^-+$|^TOTAL/ { print; next }
          /100\.00%[[:space:]]+.*100\.00%[[:space:]]+.*100\.00%[[:space:]]+.*(100\.00%|-)[[:space:]]*$/ { next }
          { print }
      '

    cargo +nightly llvm-cov report --release --lcov --output-path "$LCOV_OUT"

    # lcov 真值 plate：DA（行）/BRDA（分支）`,0` 必须为空
    miss=$(awk '/^SF:/{sf=$0} /^(DA|BRDA):.*,0$/{print sf": "$0}' "$LCOV_OUT")
    [[ -z "$miss" ]] || { printf '%s\n' "$miss"; echo "coverage gate failed: lcov DA/BRDA ,0 entries above" >&2; exit 1; }

    # 完整性自检：ignore 之外含 impl/fn 的 .rs 必须出现在 lcov SF（防漏 instrument 的虚假全绿）
    missing=$(comm -23 \
        <(fd -e rs -c never -E '*_tests.rs' -E '*_test_helpers.rs' . src/entities src/usecases src/adapters src/frameworks | sort) \
        <(grep '^SF:' "$LCOV_OUT" | sed "s|^SF:$(pwd)/||" | sort) \
      | xargs -I{} rg -l '^impl |^pub impl |^fn |^pub fn |^pub\(crate\) fn ' {} 2>/dev/null) || true
    [[ -z "$missing" ]] || { echo "coverage gate failed: production source not in lcov:" >&2; printf '%s\n' "$missing"; exit 1; }

    echo "COV_PASS"

[macos]
cov:
    @echo "cov 完整门禁仅 Linux（--all-features 需 libsmbclient）；本机可跑 just test" >&2
    @exit 1

[windows]
cov:
    @echo "cov 完整门禁仅 Linux（--all-features 需 libsmbclient）；本机可跑 just test" >&2
    @exit 1

# === fmt / lint ===
fmt:
    cargo +nightly fmt

# clippy --all-features 仅 Linux 可验；其余平台不带
[windows]
lint:
    "{{CMD}}" //c "call tools\msvc-env.cmd && cargo clippy --release --all-targets --locked --no-deps -- -D warnings"

[macos]
lint:
    cargo clippy --release --all-targets --locked --no-deps -- -D warnings

[linux]
lint:
    cargo clippy --release --all-targets --all-features --locked --no-deps -- -D warnings

# === 清理 ===
clean:
    cargo clean
