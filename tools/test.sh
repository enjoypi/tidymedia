#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"
source "$SCRIPT_DIR/cargo-env.sh"

cargo nextest run \
  "${CARGO_COMMON_FLAGS[@]}" \
  "$CARGO_LOCKED"