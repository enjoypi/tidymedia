#!/usr/bin/env bash
# cov：覆盖率门禁 = report 四指标 fail-under + lcov DA/BRDA `,0` 真值 plate（branch 唯一硬门禁）
# + 生产文件完整性自检。机制与阈值依据见 CLAUDE.md「测试与覆盖率」。
# MUST 独占跑（并行 cargo 易 SIGKILL rc=104）；跑前先 tools/fmt.sh + tools/lint.sh（行号对齐）。
# FEATURES 默认 http,sqlite，可覆盖：FEATURES=http,postgres ./tools/cov.sh
set -euo pipefail

SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"
source "$SCRIPT_DIR/cargo-env.sh"
cd "$SCRIPT_DIR/.."

LCOV_OUT=target/coverage.lcov

# --profraw-only：只清覆盖率原始数据，保留 instrumented 编译产物——重跑免全量重编
# （源码变更由 cargo 指纹自动重编）；stale 行号风险由「跑前先 fmt」顺序约束覆盖。
cargo llvm-cov clean --profraw-only

# nextest 必须：进程级单例 + env 覆盖测试在 cargo test 单进程多线程下争用必 fail
cargo +nightly llvm-cov \
  nextest \
  "${CARGO_COMMON_FLAGS[@]}" \
  "$CARGO_LOCKED" \
  --branch \
  --cargo-quiet \
  --failure-output immediate-final \
  --no-fail-fast \
  --no-report \
  --show-progress none \
  --status-level fail \
  --success-output never

# 四指标：file-lines 99.999 兜聚合 float 缺陷（全满仍 99.99999%）；其余全 100——多 instance 虚报
# 已由「擦 generic + spawn smoke + 不可达 expect 化」全量消解（见 CLAUDE.md）。
# report 不支持 --fail-under-branches，branch 由下方 lcov plate 守。输出只留 <100% 的行 + TOTAL。
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
miss=$(awk '/^SF:/{sf=$0} /^(DA|BRDA):.*,0$/ {print sf": "$0}' "$LCOV_OUT")
[[ -z "$miss" ]] || { printf '%s\n' "$miss"; echo "coverage gate failed: lcov DA/BRDA ,0 entries above" >&2; exit 1; }

# 完整性自检：ignore 之外含 impl 块/顶层 fn 的 .rs 必须出现在 lcov SF（防漏 instrument 的虚假全绿）
missing=$(comm -23 \
    <(fd -e rs -c never -E '*_tests.rs' -E '*_test_helpers.rs' . entities/src usecases/src adapters/src frameworks/src | sort) \
    <(grep '^SF:' "$LCOV_OUT" | sed "s|^SF:$(pwd)/||" | sort) \
  | xargs -I{} rg -l '^impl |^pub impl |^fn |^pub fn |^pub\(crate\) fn ' {} 2>/dev/null) || true
[[ -z "$missing" ]] || { echo "coverage gate failed: production source not in lcov (cov missing stats):" >&2; printf '%s\n' "$missing"; exit 1; }

echo "COV_PASS"
