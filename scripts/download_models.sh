#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODELS_DIR="$REPO_ROOT/models"
HF="${HF_ENDPOINT:-https://huggingface.co}"

mkdir -p "$MODELS_DIR"

echo "==> [1/1] bge-small-zh-v1.5 int8 + tokenizer"
curl --silent --show-error --fail --location \
    -o "$MODELS_DIR/bge_small_zh_v15_int8.onnx" \
    "$HF/Xenova/bge-small-zh-v1.5/resolve/main/onnx/model_quantized.onnx"
curl --silent --show-error --fail --location \
    -o "$MODELS_DIR/bge_small_zh_v15_tokenizer.json" \
    "$HF/Xenova/bge-small-zh-v1.5/resolve/main/tokenizer.json"

echo
echo "==> 模型清单"
ls -lh "$MODELS_DIR"/*.onnx "$MODELS_DIR"/*_tokenizer.json
