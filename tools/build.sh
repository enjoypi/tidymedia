#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"
source "$SCRIPT_DIR/cargo-env.sh"

cargo build \
  "$CARGO_LOCKED" \
  "${CARGO_COMMON_FLAGS[@]}"
