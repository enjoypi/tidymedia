#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"
cd "$SCRIPT_DIR/.."

LCOV_OUT=target/coverage.lcov
COV_NONBUSINESS=(
  src/adapters/backend/local_mmap.rs
  src/adapters/backend/smb_real.rs
  src/adapters/classify/tract_embed_real.rs
  src/adapters/cli_commands.rs
  src/adapters/cli_io.rs
  src/adapters/ocr/tract_dbnet_real.rs
  src/adapters/report_sink_io.rs
  src/adapters/sidecar_io.rs
  src/bin/tidymedia.rs
  src/entities/common_io.rs
  src/entities/exif/exif_io.rs
  src/entities/file_info/paths_io.rs
  src/entities/m2ts_io.rs
  src/entities/office/cfb_io.rs
  src/entities/office/flate_io.rs
  src/entities/office/plist_io.rs
  src/entities/office/read_io.rs
  src/entities/office/zip_io.rs
  src/entities/png_io.rs
  src/entities/riff_io.rs
  src/entities/threadpool.rs
  src/frameworks/backend.rs
  src/frameworks/config_io.rs
  src/frameworks/detector.rs
  src/usecases/verify/content_diff_io.rs
  src/usecases/verify/verify_io.rs
)
COV_TEST_SUPPORT=(
  src/adapters/backend/fake.rs
  src/adapters/backend/fake_ops.rs
  src/adapters/backend/fake_writers.rs
  src/adapters/classify/fake.rs
  src/adapters/classify/tract_embed_tests_common.rs
  src/adapters/ocr/fake.rs
  src/adapters/ocr/tract_dbnet_tests_common.rs
  src/entities/file_index_tests_common.rs
  src/entities/file_info/file_info_tests_common.rs
  src/entities/office/office_tests_common.rs
  src/usecases/copy/copy_tests_common.rs
  src/usecases/verify/content_diff_tests_common.rs
)
COV_EXCLUDES=("${COV_NONBUSINESS[@]}" "${COV_TEST_SUPPORT[@]}")
COV_PLATFORM_ARGS=()
COV_REPORT_ARGS=()

all_production_files() {
  fd -e rs -c never -E '*_tests.rs' -E '*_test_helpers.rs' . src | sed 's|\\|/|g' | sort
}

COV_IGNORE="$({ printf '%s\n' "${COV_EXCLUDES[@]}" | sed -e 's/\./\\./g' -e 's|/|[/\\\\]|g' | paste -sd'|' -; } | sed 's/^/(/; s/$/)$/')"

duplicates=$(printf '%s\n' "${COV_EXCLUDES[@]}" | sort | uniq -d)
[[ -z "$duplicates" ]] || { echo "coverage gate failed: duplicate exclusion entries" >&2; exit 1; }
for path in "${COV_EXCLUDES[@]}"; do
  [[ -f "$path" ]] || { echo "coverage gate failed: missing excluded file: $path" >&2; exit 1; }
  [[ "$path" != *_tests.rs ]] || { echo "coverage gate failed: test body in exclusion list: $path" >&2; exit 1; }
done
expected_ignore=$(printf '%s\n' "${COV_EXCLUDES[@]}" | sort)
actual_ignore=$(all_production_files | rg "$COV_IGNORE" || true)
[[ "$actual_ignore" == "$expected_ignore" ]] || {
  echo "coverage gate failed: exclusion regex does not match exact file list" >&2
  exit 1
}

if [[ "${COV_ZERO_IGNORE:-0}" != "1" ]]; then
  COV_REPORT_ARGS+=(--ignore-filename-regex="$COV_IGNORE")
fi

if [[ "${OS:-}" == "Windows_NT" ]]; then
  COV_PLATFORM_ARGS+=(--no-rustc-wrapper)
  COV_ROOT="$(cygpath -m "$(pwd)")"
else
  COV_ROOT="$(pwd)"
fi

cargo +nightly llvm-cov clean --profraw-only

if rg -n 'coverage\s*\(\s*off\s*\)' src; then
  echo "coverage gate failed: coverage(off) is forbidden" >&2
  exit 1
fi

cargo +nightly llvm-cov "${COV_PLATFORM_ARGS[@]}" nextest \
  --locked \
  --branch --cargo-quiet --failure-output immediate-final --no-fail-fast \
  --no-report --show-progress none --status-level fail --success-output never

cargo +nightly llvm-cov report \
  "${COV_REPORT_ARGS[@]}" \
  --lcov --output-path "$LCOV_OUT"

branch_miss=$(awk '/^SF:/{sf=$0} /^BRDA:.*,0$/{print sf": "$0}' "$LCOV_OUT")
[[ -z "$branch_miss" ]] || { printf '%s\n' "$branch_miss"; echo "coverage gate failed: branch BRDA ,0 entries above" >&2; exit 1; }

cargo +nightly llvm-cov report \
  "${COV_REPORT_ARGS[@]}" \
  --summary-only \
  --fail-under-file-lines 99.999 \
  --fail-under-functions 100 \
  --fail-under-lines 100 \
  --fail-under-regions 100

line_miss=$(awk '/^SF:/{sf=$0} /^DA:.*,0$/{print sf": "$0}' "$LCOV_OUT")
[[ -z "$line_miss" ]] || { printf '%s\n' "$line_miss"; echo "coverage gate failed: line DA ,0 entries above" >&2; exit 1; }

production_files() {
  if [[ "${COV_ZERO_IGNORE:-0}" == "1" ]]; then
    all_production_files
  else
    all_production_files | rg -v "$COV_IGNORE" || true
  fi
}

missing=$(comm -23 \
    <(production_files) \
    <(grep '^SF:' "$LCOV_OUT" | sed -e 's|\\|/|g' -e "s|^SF:${COV_ROOT}/||" -e 's|^SF:||' | sort) \
  | xargs -I{} rg -l '^impl |^pub impl |^fn |^pub fn |^pub\(crate\) fn ' {} 2>/dev/null) || true
[[ -z "$missing" ]] || { echo "coverage gate failed: production source not in lcov:" >&2; printf '%s\n' "$missing"; exit 1; }

echo "COV_PASS"
