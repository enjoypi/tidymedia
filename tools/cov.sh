#!/usr/bin/env bash
# cov：覆盖率门禁 = report 四指标 fail-under + lcov DA/BRDA `,0` 真值 plate
# + 生产文件完整性自检。MUST 独占跑；跑前先 just fmt + just lint（行号对齐）。
# Windows 必须从 just cov 进入，由 tools/msvc-env.cmd 先准备 MSVC 环境。
set -euo pipefail

SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"
cd "$SCRIPT_DIR/.."

LCOV_OUT=target/coverage.lcov
COV_IGNORE='(adapters[/\\]backend[/\\][a-z]+_real\.rs|adapters[/\\](ocr|classify)[/\\]tract_[a-z_]+\.rs|_phantom\.rs)$'
COV_PLATFORM_ARGS=()

if [[ "${OS:-}" == "Windows_NT" ]]; then
  COV_PLATFORM_ARGS+=(--no-rustc-wrapper)
  COV_ROOT="$(cygpath -m "$(pwd)")"
else
  COV_ROOT="$(pwd)"
fi

# 只清覆盖率原始数据，保留 instrumented 编译产物（重跑免全量重编）。
cargo +nightly llvm-cov clean --profraw-only

# nextest 必须：进程级单例 + env 覆盖测试在 cargo test 单进程多线程下争用必 fail。
cargo +nightly llvm-cov "${COV_PLATFORM_ARGS[@]}" nextest \
  --locked \
  --branch --cargo-quiet --failure-output immediate-final --no-fail-fast \
  --no-report --show-progress none --status-level fail --success-output never

# 第一阶段只生成 lcov，不启用其他 fail-under，确保 branch 缺口优先报告。
cargo +nightly llvm-cov report \
  --ignore-filename-regex="$COV_IGNORE" \
  --lcov --output-path "$LCOV_OUT"

# 1. branch 100%：BRDA `,0` 必须为空；失败时不再运行后续 region report。
branch_miss=$(awk '/^SF:/{sf=$0} /^BRDA:.*,0$/{print sf": "$0}' "$LCOV_OUT")
[[ -z "$branch_miss" ]] || { printf '%s\n' "$branch_miss"; echo "coverage gate failed: branch BRDA ,0 entries above" >&2; exit 1; }

# 2. region 100%（function/line 同门禁）；file-lines 99.999 兜聚合 float 缺陷。
cargo +nightly llvm-cov report \
  --ignore-filename-regex="$COV_IGNORE" \
  --summary-only \
  --fail-under-file-lines 99.999 \
  --fail-under-functions 100 \
  --fail-under-lines 100 \
  --fail-under-regions 100

# 3. lcov DA 真值 plate 复核行覆盖。
line_miss=$(awk '/^SF:/{sf=$0} /^DA:.*,0$/{print sf": "$0}' "$LCOV_OUT")
[[ -z "$line_miss" ]] || { printf '%s\n' "$line_miss"; echo "coverage gate failed: line DA ,0 entries above" >&2; exit 1; }

# 完整性自检：ignore 之外含 impl/fn 的 .rs 必须出现在 lcov SF（防漏 instrument 的虚假全绿）。
# 两侧统一为相对正斜杠路径，兼容 Windows lcov 的盘符和反斜杠。
missing=$(comm -23 \
    <(fd -e rs -c never -E '*_tests.rs' -E '*_test_helpers.rs' . src/entities src/usecases src/adapters src/frameworks | sed 's|\\|/|g' | sort | rg -v "$COV_IGNORE" || true) \
    <(grep '^SF:' "$LCOV_OUT" | sed -e 's|\\|/|g' -e "s|^SF:${COV_ROOT}/||" -e 's|^SF:||' | sort) \
  | xargs -I{} rg -l '^impl |^pub impl |^fn |^pub fn |^pub\(crate\) fn ' {} 2>/dev/null) || true
[[ -z "$missing" ]] || { echo "coverage gate failed: production source not in lcov:" >&2; printf '%s\n' "$missing"; exit 1; }

echo "COV_PASS"
